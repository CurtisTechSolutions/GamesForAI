"""Parallel game turns for batched inference and self-play."""
from __future__ import annotations

import copy
import json
import operator

import numpy as np

from ._native import NativeEnv, NativeVectorEnv
from . import curriculum
from .positions import PositionSet


class VectorEnv:
    """A batch of games. Each step advances one turn in each active row.

    Observations belong to each row's next acting seat; rewards have one column
    per seat. Finished rows receive None actions until explicitly reset.
    """

    def __init__(self, game, n, *, config=None, seed=0, threads=0):
        config_json = json.dumps(config or {})
        self.native = NativeVectorEnv(game, n, config_json, seed, threads)
        self.n = self.native.n
        self.num_players = self.native.num_players
        self.action_space_size = self.native.action_space_size
        self._prototype = NativeEnv(game, config_json)
        self._curricula = [(None, None)] * self.n

    @property
    def origins(self):
        """Public dataset identities per row; puzzle answers are excluded."""
        return [curriculum.origin_info(dataset, position_id) for dataset, position_id in self._curricula]

    def frame(self, seats=None):
        """Return owned numeric arrays; select a view per row with seats."""
        return self.native.frame(None if seats is None else list(seats))

    def step(self, actions):
        """Apply a whole batch atomically; None is required for finished rows."""
        return self.native.step(list(actions))

    def reset(self, seed=0, *, positions=None, options=None):
        """Reset all rows with seed + row, or an explicit list of seeds."""
        if isinstance(seed, (int, np.integer)):
            if isinstance(seed, (bool, np.bool_)):
                raise ValueError("seed must be an unsigned 64-bit integer")
            seeds = [int(seed) + index for index in range(self.n)]
        else:
            seeds = list(seed)
        return self.reset_at(range(self.n), seeds, positions=positions, options=options)

    def reset_at(self, indices, seeds, *, positions=None, options=None):
        """Reset selected rows, optionally sampling a pinned position set."""
        indices, seeds = list(indices), list(seeds)
        options = dict(options or {})
        if set(options) - {"position", "position_set"}:
            raise ValueError("supported reset options: position, position_set")
        if positions is not None and options:
            raise ValueError("choose positions or reset options, not both")
        if len(seeds) != len(indices):
            raise ValueError("reset seeds must match indices")
        normalized = []
        for index, seed in zip(indices, seeds):
            if isinstance(index, (bool, np.bool_)) or isinstance(seed, (bool, np.bool_)):
                raise ValueError("indices and seeds must be integers")
            try:
                index, seed = operator.index(index), operator.index(seed)
            except TypeError as error:
                raise ValueError("indices and seeds must be integers") from error
            if not 0 <= index < self.n or not 0 <= seed < 2**64:
                raise ValueError("reset index or seed is out of range")
            normalized.append((index, seed))
        if len({index for index, _ in normalized}) != len(indices):
            raise ValueError("reset indices must be unique")
        indices = [index for index, _ in normalized]
        seeds = [seed for _, seed in normalized]
        positions = None if positions is None else list(positions)
        if positions is not None and len(positions) != len(indices):
            raise ValueError("reset positions must match indices")
        if isinstance(options.get("position_set"), str):
            options["position_set"] = PositionSet.load(options["position_set"])
        starts, origins = [], self._curricula[:]
        for offset, (index, seed) in enumerate(normalized):
            row_options = {"position": positions[offset]} if positions is not None else options
            position, dataset, position_id = curriculum.resolve_start(
                self._prototype, row_options, np.random.default_rng(seed), self._curricula[index][0]
            )
            starts.append(position)
            origins[index] = dataset, position_id
        result = self.native.reset_at(indices, seeds, starts)
        self._curricula = origins
        return result

    def clone(self):
        result = copy.copy(self)
        result.native = self.native.clone()
        result._curricula = self._curricula[:]
        return result

    def get_state(self):
        """Private checkpoints; curriculum datasets are stored once per batch."""
        native = self.native.get_state()
        if not any(dataset is not None for dataset, _ in self._curricula):
            return native
        datasets, rows = {}, []
        for dataset, position_id in self._curricula:
            if dataset is None:
                rows.append(None)
            else:
                key = dataset.identifier + ":" + dataset.sha256
                datasets[key] = dataset.checkpoint()
                rows.append({"dataset": key, "position_id": position_id})
        return {"version": 2, "native": native, "datasets": datasets, "rows": rows}

    def set_state(self, snapshots):
        """Restore native state and per-row curricula as one atomic operation."""
        origins = [(None, None)] * self.n
        if isinstance(snapshots, dict):
            if (
                set(snapshots) != {"version", "native", "datasets", "rows"}
                or type(snapshots["version"]) is not int or snapshots["version"] != 2
                or not isinstance(snapshots["datasets"], dict)
                or len(snapshots["datasets"]) > self.n
                or not isinstance(snapshots["rows"], list) or len(snapshots["rows"]) != self.n
            ):
                raise ValueError("invalid vector curriculum checkpoint")
            datasets = {}
            for key, payload in snapshots["datasets"].items():
                if not isinstance(payload, dict) or set(payload) != {"manifest", "data"}:
                    raise ValueError("invalid vector position-set checkpoint")
                dataset = PositionSet.from_jsonl(payload["manifest"], payload["data"])
                dataset.validate_for(self._prototype)
                if key != dataset.identifier + ":" + dataset.sha256:
                    raise ValueError("vector checkpoint dataset identity mismatch")
                datasets[key] = dataset
            used = set()
            for index, row in enumerate(snapshots["rows"]):
                if row is None:
                    continue
                if (
                    not isinstance(row, dict) or set(row) != {"dataset", "position_id"}
                    or not isinstance(row["dataset"], str) or row["dataset"] not in datasets
                ):
                    raise ValueError("invalid vector checkpoint curriculum row")
                dataset = datasets[row["dataset"]]
                if row["position_id"] not in {entry.id for entry in dataset.entries}:
                    raise ValueError("unknown vector checkpoint position")
                origins[index] = dataset, row["position_id"]
                used.add(row["dataset"])
            if used != set(datasets):
                raise ValueError("unused vector checkpoint dataset")
            snapshots = snapshots["native"]
        if not isinstance(snapshots, (list, tuple)):
            raise ValueError("vector native checkpoints must be a list")
        candidate = self.native.clone()
        candidate.set_state(list(snapshots))
        self.native, self._curricula = candidate, origins
