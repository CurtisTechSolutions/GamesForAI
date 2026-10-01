"""Seat-visible local trajectories for supervised and reinforcement learning."""
from __future__ import annotations

import copy
import json
import math
import os
import tempfile
import uuid
from datetime import datetime, timezone
from pathlib import Path

import numpy as np

from ._native import NativeEnv


def _time(value):
    if not isinstance(value, str):
        raise ValueError("timestamp must be an ISO-8601 string with a timezone")
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("timestamp must include a timezone")
    return parsed


def _text(value, name, limit):
    if not isinstance(value, str) or not value or len(value.encode("utf-8")) > limit:
        raise ValueError(f"{name} must be nonempty text within {limit} bytes")
    return value


def _dump(value):
    return json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(",", ":"))


class EpisodeRecorder:
    """Record one native episode, one row per game turn.

    Policies receive only their own observation. The recorder keeps private
    engine state internally to make a failed move or record limit atomic.
    """

    def __init__(
        self, game, agents, *, config=None, seed=0, position=None, ratings=None,
        episode_id=None, created_at=None, max_bytes=64 * 1024 * 1024, max_steps=10000,
    ):
        self._native = NativeEnv(game, _dump(config or {}), seed)
        if position is not None:
            self._native.reset(seed, position)
        self._spec = json.loads(self._native.spec_json())
        self._agents = tuple(_text(agent, "agent id", 256) for agent in agents)
        if len(self._agents) != self._native.num_players:
            raise ValueError("one agent identity is required per seat")
        self._ratings = tuple([None] * len(self._agents) if ratings is None else ratings)
        if len(self._ratings) != len(self._agents) or any(
            rating is not None and (type(rating) not in (int, float) or not math.isfinite(rating))
            for rating in self._ratings
        ):
            raise ValueError("ratings must be finite values or None, one per seat")
        if type(max_bytes) is not int or not 1 <= max_bytes <= 256 * 1024 * 1024:
            raise ValueError("max_bytes must be 1..256 MiB")
        if type(max_steps) is not int or not 1 <= max_steps <= 100000:
            raise ValueError("max_steps must be 1..100000")
        snapshot = json.loads(self._native.get_state())
        self._metadata = {
            "format_version": 1,
            "episode_id": _text(episode_id or str(uuid.uuid4()), "episode id", 256),
            "game": game,
            "engine_version": snapshot["engine_version"],
            "config_json": _dump(snapshot["config"]),
            "seed": str(seed),
            "created_at": created_at or datetime.now(timezone.utc).isoformat(),
        }
        _time(self._metadata["created_at"])
        self._rows = []
        self._bytes = 0
        self._max_bytes, self._max_steps = max_bytes, max_steps

    @property
    def current_players(self):
        return self._native.current_players()

    @property
    def flags(self):
        return self._native.flags()

    @property
    def returns(self):
        return self._native.returns()

    def observation(self, seat):
        """Inputs usable by Python callbacks or ChatPolicy without private state."""
        frame = self._native.frame(seat)
        terminated, truncated = self._native.flags()
        info = {
            "game_id": self._spec["id"], "rules": self._spec["rules_markdown"],
            "action_notation": self._spec["action_notation"], "seat": seat,
            "to_act": self.current_players, "terminated": terminated, "truncated": truncated,
            "action_mask": frame["action_mask"], "legal_actions": frame["legal_actions"],
            "text": frame["text"], "board": json.loads(frame["board_json"]),
        }
        return frame["observation"], info

    def step(self, seat, action, *, reasoning=None, transcript=None):
        if isinstance(seat, (bool, np.bool_)) or not isinstance(seat, (int, np.integer)):
            raise ValueError("seat must be an integer")
        if isinstance(action, (bool, np.bool_)) or not isinstance(action, (int, np.integer)):
            raise ValueError("action must be an integer index")
        seat, action = int(seat), int(action)
        if reasoning is not None:
            _text(reasoning, "reasoning", 64 * 1024)
        transcript_json = None if transcript is None else _dump(transcript)
        if transcript_json is not None and len(transcript_json.encode("utf-8")) > 1024 * 1024:
            raise ValueError("transcript exceeds 1 MiB")
        if len(self._rows) >= self._max_steps:
            raise ValueError("episode record step limit reached")
        before = self._native.frame(seat)
        catalog = dict(before["legal_actions"])
        if action not in catalog:
            raise ValueError("recorded action must be legal for the acting seat")
        candidate = self._native.clone()
        rewards, terminated, truncated = candidate.step(seat, action)
        after = candidate.frame(seat)
        row = {
            **self._metadata,
            "ply": len(self._rows), "seat": seat, "agent": self._agents[seat],
            "agent_rating": self._ratings[seat],
            "observation": before["observation"].reshape(-1).tolist(),
            "observation_shape": list(before["observation"].shape),
            "action_mask": before["action_mask"].astype(bool).tolist(),
            "observation_text": before["text"],
            "action": catalog[action], "action_index": action,
            "reasoning": reasoning, "transcript_json": transcript_json,
            "rewards": list(rewards), "reward": float(rewards[seat]),
            "next_observation": after["observation"].reshape(-1).tolist(),
            "next_action_mask": after["action_mask"].astype(bool).tolist(),
            "next_to_act": candidate.current_players(),
            "terminated": terminated, "truncated": truncated,
        }
        # Include final fields at their maximum serialized size in the bound.
        size = len(_dump({**row, "outcome": "truncated", "episode_returns": [-1.0] * len(rewards)}).encode("utf-8")) + 1
        if self._bytes + size > self._max_bytes:
            raise ValueError("episode record byte limit reached")
        self._native = candidate
        self._rows.append(row)
        self._bytes += size
        return list(rewards), terminated, truncated

    def records(self):
        """Finalized rows with the eventual outcome for each acting seat."""
        terminated, truncated = self.flags
        if not terminated and not truncated:
            raise ValueError("finish the episode before exporting its outcomes")
        returns = self.returns
        result = []
        for row in self._rows:
            value = returns[row["seat"]]
            outcome = "truncated" if truncated else "win" if value > 0 else "loss" if value < 0 else "draw"
            result.append({**copy.deepcopy(row), "outcome": outcome, "episode_returns": returns[:]})
        return result


def select_records(
    records, *, game=None, agent=None, min_rating=None, max_rating=None,
    after=None, before=None, outcome=None,
):
    """Filter completed records; timestamps use [after, before)."""
    start, end = None if after is None else _time(after), None if before is None else _time(before)
    if start is not None and end is not None and start >= end:
        raise ValueError("date filter must have after < before")
    for rating in (min_rating, max_rating):
        if rating is not None and (type(rating) not in (int, float) or not math.isfinite(rating)):
            raise ValueError("rating bounds must be finite")
    if min_rating is not None and max_rating is not None and min_rating > max_rating:
        raise ValueError("minimum rating exceeds maximum")
    if outcome is not None and outcome not in {"win", "loss", "draw", "truncated"}:
        raise ValueError("unknown trajectory outcome")
    for row in records:
        if game is not None and row["game"] != game:
            continue
        if agent is not None and row["agent"] != agent:
            continue
        if outcome is not None and row["outcome"] != outcome:
            continue
        rating = row["agent_rating"]
        if min_rating is not None and (rating is None or rating < min_rating):
            continue
        if max_rating is not None and (rating is None or rating > max_rating):
            continue
        timestamp = _time(row["created_at"])
        if (start is not None and timestamp < start) or (end is not None and timestamp >= end):
            continue
        yield copy.deepcopy(row)


def write_jsonl(path, records):
    """Atomically replace a JSONL file; a failed generator preserves old output."""
    path = Path(path)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="\n", dir=path.parent, delete=False) as file:
            temporary = Path(file.name)
            count = 0
            for row in records:
                if row.get("format_version") != 1 or row.get("outcome") not in {"win", "loss", "draw", "truncated"}:
                    raise ValueError("export requires completed version 1 trajectory records")
                file.write(_dump(row) + "\n")
                count += 1
            file.flush()
            os.fsync(file.fileno())
        os.replace(temporary, path)
        return count
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
