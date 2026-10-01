"""Shared native Glicko-2 calculations for local model leagues."""
from __future__ import annotations

import json
from dataclasses import asdict, dataclass

from ._native import rating_pair_json, rating_period_json, rating_validate_json


def _dump(value):
    return json.dumps(value, allow_nan=False)


@dataclass(frozen=True)
class Rating:
    rating: float = 1500.0
    rd: float = 350.0
    volatility: float = 0.06
    games_played: int = 0

    def __post_init__(self):
        rating_validate_json(_dump(asdict(self)))

    @property
    def interval95(self):
        """Approximate rating interval; this is not a win-rate confidence bound."""
        return (self.rating - 2 * self.rd, self.rating + 2 * self.rd)

    def update(self, results=(), *, tau=0.5):
        """Apply one period from (pre-period opponent Rating, score) pairs."""
        payload = [(asdict(opponent), score) for opponent, score in results]
        return Rating(**json.loads(rating_period_json(_dump(asdict(self)), _dump(payload), tau)))

    def to_dict(self):
        return asdict(self)


def rate_pair(left, right, score, *, fixed_left=False, fixed_right=False, tau=0.5):
    """Return both new ratings without mutating either pre-match rating."""
    if isinstance(score, bool):
        raise ValueError("score must be numeric 0, 0.5 or 1")
    if type(fixed_left) is not bool or type(fixed_right) is not bool:
        raise ValueError("fixed anchor flags must be boolean")
    result = json.loads(rating_pair_json(
        _dump(asdict(left)), _dump(asdict(right)), score, fixed_left, fixed_right, tau
    ))
    return tuple(Rating(**rating) for rating in result)
