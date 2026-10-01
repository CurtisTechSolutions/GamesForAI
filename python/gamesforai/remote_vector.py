"""Atomic local batches over the stateless remote training endpoint."""
from __future__ import annotations

import copy
import json
import math

import numpy as np

from .remote import RemoteError, _code, _integer
from .http_policy import _json


class RemoteNativeVectorEnv:
    def __init__(self, client, game, n, config_json="{}", seed=0, threads=0, *, batch_size=32):
        self._client = client
        self.n = _integer(n, 4096)
        self._batch_size = _integer(batch_size, 64)
        if not self.n or not self._batch_size:
            raise ValueError("n must be 1..4096 and batch_size 1..64")
        if _integer(threads, 64) != 0:
            raise ValueError("remote workers are controlled by the server; leave threads=0")
        seed = _integer(seed, 2**64 - self.n)
        prototype = client.native(game, config_json, seed)
        self.num_players = prototype.num_players
        self.action_space_size = prototype.action_space_size
        self._shape = prototype._shape
        row_bytes = math.prod(self._shape) * 4 + self.action_space_size + self.num_players * 8 + 6
        if row_bytes * self.n > 128 * 1024 * 1024:
            raise ValueError("batch numeric arrays exceed 128 MiB")
        candidate = [prototype] + [prototype.clone() for _ in range(self.n - 1)]
        operations = [
            (index, 0, {"op": "create", "game_id": game, "config": _json(config_json),
                        "seed": seed + index, "seat": 0})
            for index in range(1, self.n)
        ]
        self._run(candidate, operations)
        self._project(candidate, None)
        self._envs = candidate

    def _chunks(self, jobs):
        batch, size = [], 32
        for job in jobs:
            length = len(json.dumps(job[2], allow_nan=False, separators=(",", ":")).encode()) + 1
            if length + 32 > 4 * 1024 * 1024:
                raise RemoteError("REQUEST_TOO_LARGE")
            if batch and (len(batch) >= self._batch_size or size + length > 4 * 1024 * 1024):
                yield batch
                batch, size = [], 32
            batch.append(job)
            size += length
        if batch:
            yield batch

    def _run(self, candidate, jobs, rewards=None, *, unchanged=False):
        for chunk in self._chunks(jobs):
            results = self._client.batch([job[2] for job in chunk])
            for (index, seat, _), result in zip(chunk, results):
                if result["status"] == "error":
                    raise RemoteError(_code(result))
                candidate[index]._accept(result, seat, unchanged=unchanged)
                if rewards is not None:
                    rewards[index] = result["frame"]["rewards"]
                if candidate[index].num_players != self.num_players or candidate[index]._shape != self._shape:
                    raise RemoteError("INVALID_RESPONSE")

    def _seats(self, candidate, seats):
        if seats is not None and len(seats) != self.n:
            raise ValueError("seat count must equal batch size")
        result = []
        for index, env in enumerate(candidate):
            actors = env.current_players()
            if len(actors) > 1:
                raise ValueError("VectorEnv requires sequential turns")
            result.append((actors[0] if actors else 0) if seats is None
                          else _integer(seats[index], self.num_players - 1))
        return result

    def _project(self, candidate, seats):
        seats = self._seats(candidate, seats)
        jobs = [
            (index, seat, {"op": "observe", "checkpoint": env._checkpoint, "seat": seat})
            for index, (env, seat) in enumerate(zip(candidate, seats))
            if seat not in env._frames
        ]
        self._run(candidate, jobs, unchanged=True)
        # Vector inference needs one view per row; keep Python mask storage bounded.
        for env, seat in zip(candidate, seats):
            env._frames = {seat: env._frames[seat]}
        return seats

    def _arrays(self, candidate, seats=None, rewards=None):
        seats = self._project(candidate, seats)
        output = {
            "observation": np.empty((self.n, *self._shape), dtype=np.float32),
            "action_mask": np.empty((self.n, self.action_space_size), dtype=np.int8),
            "seat": np.array(seats, dtype=np.int16),
            "to_act": np.empty(self.n, dtype=np.int16),
            "terminated": np.empty(self.n, dtype=bool),
            "truncated": np.empty(self.n, dtype=bool),
            "rewards": np.zeros((self.n, self.num_players), dtype=np.float64) if rewards is None else rewards.copy(),
        }
        for index, (env, seat) in enumerate(zip(candidate, seats)):
            raw = env._frames[seat]
            tensor = raw["observation"]["tensor"]
            output["observation"][index] = np.array(tensor["values"], dtype=np.float32).reshape(self._shape)
            output["action_mask"][index] = raw["action_mask"]
            actors = raw["to_act"]
            output["to_act"][index] = actors[0] if actors else -1
            output["terminated"][index] = raw["terminated"]
            output["truncated"][index] = raw["truncated"]
        return output

    def frame(self, seats=None):
        candidate = [env.clone() for env in self._envs]
        result = self._arrays(candidate, seats)
        self._envs = candidate
        return result

    def step(self, actions):
        if len(actions) != self.n:
            raise ValueError("action count must equal batch size")
        candidate = [env.clone() for env in self._envs]
        jobs = []
        for index, (env, action) in enumerate(zip(candidate, actions)):
            actors = env.current_players()
            if not actors and any(env.flags()) and action is None:
                continue
            if len(actors) != 1 or action is None:
                raise ValueError("use legal indices for active rows and None for finished rows")
            seat = actors[0]
            jobs.append((index, seat, {"op": "step", "checkpoint": env._checkpoint,
                                      "seat": seat, "action": _integer(action, self.action_space_size - 1)}))
        rewards = np.zeros((self.n, self.num_players), dtype=np.float64)
        self._run(candidate, jobs, rewards)
        result = self._arrays(candidate, rewards=rewards)
        self._envs = candidate
        return result

    def reset_at(self, indices, seeds, positions):
        if len(indices) != len(seeds) or len(indices) != len(positions):
            raise ValueError("indices, seeds, and positions must have equal lengths")
        candidate = [env.clone() for env in self._envs]
        jobs, seen = [], set()
        for index, seed, position in zip(indices, seeds, positions):
            index, seed = _integer(index, self.n - 1), _integer(seed, 2**64 - 1)
            if index in seen or (position is not None and not isinstance(position, str)):
                raise ValueError("indices must be unique and positions must be strings or None")
            seen.add(index)
            jobs.append((index, 0, {"op": "reset", "checkpoint": candidate[index]._checkpoint,
                                   "seed": seed, "position": position, "seat": 0}))
        self._run(candidate, jobs)
        result = self._arrays(candidate)
        self._envs = candidate
        return result

    def clone(self):
        result = copy.copy(self)
        result._envs = [env.clone() for env in self._envs]
        return result

    def get_state(self):
        return [env.get_state() for env in self._envs]

    def set_state(self, snapshots):
        if len(snapshots) != self.n or any(not isinstance(item, str) for item in snapshots):
            raise ValueError("checkpoint count or type is invalid")
        if sum(len(item.encode()) for item in snapshots) > 128 * 1024 * 1024:
            raise ValueError("batch checkpoints exceed 128 MiB")
        candidate, jobs = [env.clone() for env in self._envs], []
        for index, (env, snapshot) in enumerate(zip(candidate, snapshots)):
            checkpoint = _json(snapshot)
            if (not isinstance(checkpoint, dict) or checkpoint.get("game_id") != env.game_id
                    or checkpoint.get("config") != env._checkpoint["config"]):
                raise ValueError("checkpoint must match game and configuration")
            jobs.append((index, 0, {"op": "observe", "checkpoint": checkpoint, "seat": 0}))
        self._run(candidate, jobs)
        self._project(candidate, None)
        self._envs = candidate
