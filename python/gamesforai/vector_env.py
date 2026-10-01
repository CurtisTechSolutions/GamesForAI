"""Parallel game turns for batched inference and self-play."""
from __future__ import annotations

import copy
import json

from ._native import NativeVectorEnv


class VectorEnv:
    """A batch of games. Each step advances one turn in each active row.

    Observations belong to each row's next acting seat; rewards have one column
    per seat. Finished rows receive None actions until explicitly reset.
    """

    def __init__(self, game, n, *, config=None, seed=0, threads=0):
        self.native = NativeVectorEnv(game, n, json.dumps(config or {}), seed, threads)
        self.n = self.native.n
        self.num_players = self.native.num_players
        self.action_space_size = self.native.action_space_size

    def frame(self, seats=None):
        """Return owned numeric arrays; select a view per row with seats."""
        return self.native.frame(None if seats is None else list(seats))

    def step(self, actions):
        """Apply a whole batch atomically; None is required for finished rows."""
        return self.native.step(list(actions))

    def reset(self, seed=0, *, positions=None):
        """Reset every row using seed + row, or an explicit list of seeds."""
        seeds = [seed + index for index in range(self.n)] if isinstance(seed, int) else list(seed)
        return self.native.reset_at(
            list(range(self.n)), seeds, [None] * self.n if positions is None else list(positions)
        )

    def reset_at(self, indices, seeds, *, positions=None):
        """Reset selected rows without discarding other episodes."""
        indices = list(indices)
        return self.native.reset_at(
            indices, list(seeds), [None] * len(indices) if positions is None else list(positions)
        )

    def clone(self):
        result = copy.copy(self)
        result.native = self.native.clone()
        return result

    def get_state(self):
        """Full private checkpoints for trusted training code."""
        return self.native.get_state()

    def set_state(self, snapshots):
        self.native.set_state(list(snapshots))
