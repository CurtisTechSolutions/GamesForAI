"""Caller-owned training environments over the GamesForAI REST API."""
from __future__ import annotations

import copy
import json
import math
import re
import socket
from http.client import HTTPException
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlsplit
from urllib.request import Request, build_opener

import numpy as np

from .http_policy import _NoRedirect, _json


class RemoteError(ValueError):
    """Stable transport/domain failure without URLs, credentials or raw response text."""

    def __init__(self, code, status=None):
        self.code, self.status = code, status
        super().__init__(f"remote environment failed: {code}")


def _integer(value, maximum):
    if isinstance(value, (bool, np.bool_)) or not isinstance(value, (int, np.integer)):
        raise ValueError("expected an integer")
    value = int(value)
    if not 0 <= value <= maximum:
        raise ValueError("integer is outside the supported range")
    return value


def _code(body):
    error = body.get("error") if isinstance(body, dict) else None
    value = error.get("code") if isinstance(error, dict) else None
    return value if isinstance(value, str) and re.fullmatch(r"[A-Z][A-Z0-9_]{0,63}", value) else "REMOTE_ERROR"


class Client:
    """Explicit connection configuration; no server environment handles are retained."""

    def __init__(self, url, api_key=None, *, timeout=60):
        parsed = urlsplit(url)
        if (parsed.scheme not in ("http", "https") or not parsed.hostname
                or parsed.username or parsed.password or parsed.query or parsed.fragment):
            raise ValueError("url must be an HTTP(S) server root without credentials or query")
        if api_key is not None and (not isinstance(api_key, str) or not api_key
                                   or len(api_key) > 4096 or "\r" in api_key or "\n" in api_key):
            raise ValueError("api_key must be nonempty single-line text")
        if isinstance(timeout, bool) or not isinstance(timeout, (int, float)) or not math.isfinite(timeout) or not 0 < timeout <= 300:
            raise ValueError("timeout must be in (0, 300] seconds")
        self._url, self._key, self.timeout = url.rstrip("/"), api_key, timeout
        self._specs = {}

    def __repr__(self):
        return "<GamesForAI Client>"

    def _request(self, path, body=None):
        encoded = None if body is None else json.dumps(body, allow_nan=False, separators=(",", ":")).encode()
        if encoded is not None and len(encoded) > 4 * 1024 * 1024:
            raise RemoteError("REQUEST_TOO_LARGE")
        headers = {"Accept": "application/json", "Content-Type": "application/json"}
        if self._key is not None:
            headers["Authorization"] = "Bearer " + self._key
        request = Request(self._url + path, data=encoded, headers=headers)
        try:
            with build_opener(_NoRedirect()).open(request, timeout=self.timeout) as response:
                raw = response.read(8 * 1024 * 1024 + 1)
            if len(raw) > 8 * 1024 * 1024:
                raise RemoteError("RESPONSE_TOO_LARGE")
            return _json(raw)
        except HTTPError as error:
            try:
                raw = error.read(65537)
                body = _json(raw) if len(raw) <= 65536 else None
                code = _code(body)
            except Exception:
                code = "HTTP_ERROR"
            finally:
                error.close()
            raise RemoteError(code, error.code) from None
        except (socket.timeout, TimeoutError):
            raise RemoteError("TIMEOUT") from None
        except URLError as error:
            raise RemoteError("TIMEOUT" if isinstance(error.reason, (socket.timeout, TimeoutError)) else "UNAVAILABLE") from None
        except (HTTPException, OSError):
            raise RemoteError("UNAVAILABLE") from None
        except (ValueError, RecursionError, UnicodeError) as error:
            if isinstance(error, RemoteError):
                raise
            raise RemoteError("INVALID_RESPONSE") from None

    def games(self):
        specs = self._request("/v1/games")
        if not isinstance(specs, list) or any(not isinstance(spec, dict) or not isinstance(spec.get("id"), str) for spec in specs):
            raise RemoteError("INVALID_RESPONSE")
        return [spec["id"] for spec in specs]

    def _spec(self, game):
        if not isinstance(game, str) or not game or len(game) > 128:
            raise ValueError("game must be a nonempty identifier within 128 characters")
        if game not in self._specs:
            spec = self._request("/v1/games/" + quote(game, safe=""))
            if (not isinstance(spec, dict) or spec.get("id") != game
                    or not isinstance(spec.get("engine_version"), str)):
                raise RemoteError("INVALID_RESPONSE")
            self._specs[game] = spec
        return copy.deepcopy(self._specs[game])

    def batch(self, operations):
        """Return independent row results; no local states are automatically committed."""
        operations = list(operations)
        if not 1 <= len(operations) <= 64:
            raise ValueError("a batch requires 1..64 operations")
        response = self._request("/v1/batch/step", {"operations": operations})
        if not isinstance(response, dict) or not isinstance(response.get("results"), list) or len(response["results"]) != len(operations):
            raise RemoteError("INVALID_RESPONSE")
        for result in response["results"]:
            if not isinstance(result, dict) or result.get("status") not in ("ok", "error"):
                raise RemoteError("INVALID_RESPONSE")
        return response["results"]

    def native(self, game, config_json="{}", seed=0):
        return RemoteNativeEnv(self, game, config_json, seed)

    def make(self, game, **kwargs):
        from .gym_env import GameEnv
        if kwargs.get("stockfish_pool") is not None or str(kwargs.get("opponent", "")).startswith("stockfish:"):
            raise ValueError("remote Gym currently supports random, minimax/mcts or a Python/HTTP policy")
        return GameEnv(game, _native_factory=self.native, **kwargs)

    def vector(self, game, n, *, batch_size=32, **kwargs):
        from .remote_vector import RemoteNativeVectorEnv
        from .vector_env import VectorEnv

        def factory(game, n, config_json, seed, threads):
            return RemoteNativeVectorEnv(self, game, n, config_json, seed, threads,
                                         batch_size=batch_size)
        return VectorEnv(game, n, _native_factory=factory, _prototype_factory=self.native, **kwargs)

    def aec(self, game, **kwargs):
        from .aec_env import AECGameEnv
        return AECGameEnv(game, _native_factory=self.native, **kwargs)


class RemoteNativeEnv:
    """NativeEnv-compatible controller with exact private checkpoints held locally."""

    def __init__(self, client, game, config_json="{}", seed=0):
        self._client, self.game_id = client, game
        self._spec = client._spec(game)
        self.action_space_size = _integer(self._spec["action_space_size"], 1000000)
        if not self.action_space_size:
            raise RemoteError("INVALID_RESPONSE")
        self._checkpoint = None
        self._frames = {}
        self._shape = None
        self.num_players = None
        result = self._one({"op": "create", "game_id": game, "config": _json(config_json),
                            "seed": _integer(seed, 2**64 - 1), "seat": 0})
        self._commit(result, 0)

    def _one(self, operation):
        result = self._client.batch([operation])[0]
        if result["status"] == "error":
            raise RemoteError(_code(result))
        return result

    def _accept(self, result, seat, *, unchanged=False):
        try:
            checkpoint, frame = result["checkpoint"], result["frame"]
            if (checkpoint["format_version"] != 1 or checkpoint["game_id"] != self.game_id
                    or checkpoint["engine_version"] != self._spec["engine_version"] or frame["seat"] != seat):
                raise ValueError()
            if self._checkpoint is not None and checkpoint["config"] != self._checkpoint["config"]:
                raise ValueError()
            if unchanged and checkpoint != self._checkpoint:
                raise ValueError()
            returns, rewards, actors = frame["returns"], frame["rewards"], frame["to_act"]
            count = len(returns)
            if not 1 <= count <= 255 or (self.num_players is not None and count != self.num_players):
                raise ValueError()
            if len(rewards) != count or any(type(x) not in (int, float) or not math.isfinite(x) for x in returns + rewards):
                raise ValueError()
            if (not isinstance(actors, list) or len(set(actors)) != len(actors)
                    or any(type(actor) is not int or not 0 <= actor < count for actor in actors)):
                raise ValueError()
            if type(frame["terminated"]) is not bool or type(frame["truncated"]) is not bool:
                raise ValueError()
            mask, legal = frame["action_mask"], frame["legal_actions"]
            if len(mask) != self.action_space_size or any(type(value) is not bool for value in mask):
                raise ValueError()
            indices = [item["index"] for item in legal]
            if (len(indices) != len(set(indices))
                    or any(type(index) is not int or not 0 <= index < len(mask) for index in indices)
                    or set(indices) != {i for i, value in enumerate(mask) if value}
                    or any(not isinstance(item["string"], str) for item in legal)):
                raise ValueError()
            obs = frame["observation"]
            if not isinstance(obs["text"], str) or "json" not in obs:
                raise ValueError()
            shape = self._shape
            if seat is not None:
                tensor = obs["tensor"]
                shape, values = tensor["shape"], tensor["values"]
                if (not isinstance(shape, list) or not 1 <= len(shape) <= 8
                        or any(type(x) is not int or not 1 <= x <= 1000000 for x in shape)
                        or math.prod(shape) != len(values) or len(values) > 1000000
                        or (self._shape is not None and tuple(shape) != self._shape)):
                    raise ValueError()
                array = np.array(values, dtype=np.float32)
                if not np.all(np.isfinite(array)):
                    raise ValueError()
                shape = tuple(shape)
            if (frame["terminated"] or frame["truncated"]) and actors:
                raise ValueError()
        except (KeyError, TypeError, ValueError, OverflowError):
            raise RemoteError("INVALID_RESPONSE") from None
        # Validate everything before committing a transition or checkpoint restore.
        if not unchanged:
            self._frames = {}
        self._checkpoint = copy.deepcopy(checkpoint)
        self._frames[seat] = copy.deepcopy(frame)
        self.num_players, self._shape = count, shape

    def _commit(self, result, seat):
        # Fetch all player frames before changing state. AEC bookkeeping can then
        # refresh every player's info without a new network failure mid-step.
        candidate = self.clone()
        candidate._accept(result, seat)
        missing = [value for value in range(candidate.num_players) if value != seat]
        for offset in range(0, len(missing), 64):
            seats = missing[offset:offset + 64]
            results = self._client.batch([
                {"op": "observe", "checkpoint": candidate._checkpoint, "seat": value}
                for value in seats
            ])
            for value, response in zip(seats, results):
                if response["status"] == "error":
                    raise RemoteError(_code(response))
                candidate._accept(response, value, unchanged=True)
        self._checkpoint, self._frames = candidate._checkpoint, candidate._frames
        self.num_players, self._shape = candidate.num_players, candidate._shape

    def _raw_frame(self, seat):
        if seat is not None:
            seat = _integer(seat, self.num_players - 1)
        if seat not in self._frames:
            self._accept(self._one({"op": "observe", "checkpoint": self._checkpoint, "seat": seat}),
                         seat, unchanged=True)
        return self._frames[seat]

    def reset(self, seed=0, position=None):
        if position is not None and not isinstance(position, str):
            raise ValueError("position must be text")
        self._commit(self._one({"op": "reset", "checkpoint": self._checkpoint,
                               "seed": _integer(seed, 2**64 - 1), "position": position, "seat": 0}), 0)

    def step(self, seat, action):
        seat = _integer(seat, self.num_players - 1)
        action = _integer(action, self.action_space_size - 1)
        self._commit(self._one({"op": "step", "checkpoint": self._checkpoint,
                               "seat": seat, "action": action}), seat)
        frame = self._frames[seat]
        return frame["rewards"][:], frame["terminated"], frame["truncated"]

    def step_string(self, seat, action):
        for item in self._raw_frame(seat)["legal_actions"]:
            if item["string"] == action:
                return self.step(seat, item["index"])
        raise ValueError("action must be a canonical legal move")

    def builtin_action(self, seat, algorithm, level, seed):
        if algorithm not in ("minimax", "mcts"):
            raise ValueError("builtin algorithm must be minimax or mcts")
        seat, level, seed = _integer(seat, self.num_players - 1), _integer(level, 10), _integer(seed, 2**64 - 1)
        if level == 0:
            raise ValueError("level must be 1..10")
        response = self._client._request(f"/v1/analysis?seat={seat}", {
            "game_id": self.game_id, "config": self._checkpoint["config"],
            "from": {"state": self._checkpoint["state"]},
            "opponent": {"id": algorithm, "level": level}, "seed": seed,
        })
        try:
            action = response["best_moves"][0]["index"]
            if type(action) is not int or not 0 <= action < self.action_space_size or not self._raw_frame(seat)["action_mask"][action]:
                raise ValueError()
        except (KeyError, IndexError, TypeError, ValueError):
            raise RemoteError("INVALID_RESPONSE") from None
        return action

    def action_mask(self, seat):
        return np.array(self._raw_frame(seat)["action_mask"], dtype=bool)

    def spec_json(self):
        return json.dumps(self._spec)

    def current_players(self):
        return next(iter(self._frames.values()))["to_act"][:]

    def flags(self):
        frame = next(iter(self._frames.values()))
        return frame["terminated"], frame["truncated"]

    def returns(self):
        return next(iter(self._frames.values()))["returns"][:]

    def frame(self, seat):
        raw = self._raw_frame(seat)
        obs = raw["observation"]
        return {
            "observation": np.array(obs["tensor"]["values"], dtype=np.float32).reshape(obs["tensor"]["shape"]),
            "action_mask": np.array(raw["action_mask"], dtype=np.int8),
            "legal_actions": [(item["index"], item["string"]) for item in raw["legal_actions"]],
            "text": obs["text"], "board_json": json.dumps(obs["json"]),
        }

    def public_text(self):
        return self._raw_frame(None)["observation"]["text"]

    def clone(self):
        result = copy.copy(self)
        result._checkpoint, result._frames = copy.deepcopy(self._checkpoint), copy.deepcopy(self._frames)
        return result

    def get_state(self):
        return json.dumps(self._checkpoint, allow_nan=False)

    def set_state(self, snapshot):
        checkpoint = _json(snapshot)
        if (not isinstance(checkpoint, dict) or checkpoint.get("game_id") != self.game_id
                or checkpoint.get("config") != self._checkpoint["config"]):
            raise ValueError("checkpoint must match game and configuration")
        result = self._one({"op": "observe", "checkpoint": checkpoint, "seat": 0})
        self._commit(result, 0)


def connect(url, api_key=None, *, timeout=60):
    """Connect to a GamesForAI server root; make(), aec() and native() use REST."""
    return Client(url, api_key, timeout=timeout)
