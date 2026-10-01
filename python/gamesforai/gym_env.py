"""Gymnasium interface for a policy controlling one seat."""
from __future__ import annotations

import copy
import json
import re
from collections.abc import Callable
from typing import Any

import gymnasium as gym
import numpy as np
from gymnasium import spaces
from gymnasium.error import ResetNeeded

from ._native import NativeEnv
from . import curriculum
from .stockfish import StockfishPool

Policy = Callable[[np.ndarray, dict[str, Any]], int]


class GameEnv(gym.Env):
    """One learner seat against random opponents or a caller-supplied policy.

    A callable opponent receives only its own observation and info (including
    the legal mask). Its external model state remains the caller's responsibility.
    """

    metadata = {"render_modes": ["ansi"]}

    def __init__(
        self,
        game: str,
        *,
        config: dict[str, Any] | None = None,
        seat: int = 0,
        opponent: str | Policy = "random",
        stockfish_pool: StockfishPool | None = None,
        render_mode: str | None = None,
        _native_factory=NativeEnv,
    ) -> None:
        if render_mode not in (None, "ansi"):
            raise ValueError("render_mode must be None or 'ansi'")
        self._builtin = None
        self._stockfish = None
        self._stockfish_level = None
        if isinstance(opponent, str) and opponent != "random":
            match = re.fullmatch(r"(minimax|mcts|stockfish):([1-9]|10)", opponent)
            if not match:
                raise ValueError("opponent must be random, minimax:1..10, mcts:1..10, stockfish:1..10, or a callable")
            if match[1] == "stockfish":
                if game != "chess":
                    raise ValueError("Stockfish requires a chess environment")
                if stockfish_pool is not None and not isinstance(stockfish_pool, StockfishPool):
                    raise ValueError("stockfish_pool must be a StockfishPool")
                self._stockfish = stockfish_pool or StockfishPool()
                self._stockfish_level = int(match[2])
            else:
                self._builtin = match[1], int(match[2])
        elif opponent != "random" and not callable(opponent):
            raise ValueError("opponent must be a supported name or Python policy callable")
        if stockfish_pool is not None and self._stockfish is None:
            raise ValueError("stockfish_pool requires a stockfish opponent")
        self.native = _native_factory(game, json.dumps(config or {}))
        if isinstance(seat, bool) or not isinstance(seat, int) or not 0 <= seat < self.native.num_players:
            raise ValueError("seat is outside this game's player range")
        self._spec = json.loads(self.native.spec_json())
        if self._builtin and (
            self._spec["information"] != "perfect" or self._spec["stochastic"]
            or self._spec["turn_structure"] != "sequential"
            or (self._builtin[0] == "minimax" and self._spec["num_players"] != [2, 2])
        ):
            raise ValueError("builtin search does not support this game")
        self.seat = seat
        self.opponent = opponent
        self.render_mode = render_mode
        frame = self.native.frame(seat)
        self.action_space = spaces.Discrete(self.native.action_space_size)
        self.observation_space = spaces.Box(
            -np.inf, np.inf, shape=frame["observation"].shape, dtype=np.float32
        )
        self._needs_reset = True
        self._position_set = None
        self._position_id = None

    def _frame(self, seat: int) -> tuple[np.ndarray, dict[str, Any]]:
        frame = self.native.frame(seat)
        terminated, truncated = self.native.flags()
        info = {
            "action_mask": frame["action_mask"],
            "legal_actions": frame["legal_actions"],
            "game_id": self._spec["id"],
            "rules": self._spec["rules_markdown"],
            "action_notation": self._spec["action_notation"],
            "text": frame["text"],
            "board": json.loads(frame["board_json"]),
            "seat": seat,
            "to_act": self.native.current_players(),
            "terminated": terminated,
            "truncated": truncated,
            **curriculum.origin_info(self._position_set, self._position_id),
        }
        return frame["observation"], info

    def _opponents(self) -> float:
        reward = 0.0
        while actors := self.native.current_players():
            if len(actors) != 1:
                raise ValueError("Gym single-seat play requires sequential turns; use a multi-agent interface")
            seat = actors[0]
            if seat == self.seat:
                break
            observation, info = self._frame(seat)
            if self._stockfish is not None:
                seed = int(self.np_random.integers(0, 2**64, dtype=np.uint64))
                action = self._stockfish.choose(self.native, seat, self._stockfish_level, seed)
            elif self._builtin:
                algorithm, level = self._builtin
                seed = int(self.np_random.integers(0, 2**64, dtype=np.uint64))
                action = self.native.builtin_action(seat, algorithm, level, seed)
            elif callable(self.opponent):
                action = self.opponent(observation, info)
                if isinstance(action, (bool, np.bool_)) or not isinstance(action, (int, np.integer)):
                    raise ValueError("opponent policy must return an integer action index")
                action = int(action)
            else:
                legal = np.flatnonzero(info["action_mask"])
                if not legal.size:
                    raise ValueError("active opponent has no legal action")
                action = int(self.np_random.choice(legal))
            rewards, _, _ = self.native.step(seat, action)
            reward += rewards[self.seat]
        return reward

    def reset(
        self, *, seed: int | None = None, options: dict[str, Any] | None = None
    ) -> tuple[np.ndarray, dict[str, Any]]:
        options = options or {}
        if set(options) - {"position", "position_set"}:
            raise ValueError("supported reset options: position, position_set")
        saved_native = self.native
        saved_curriculum = self._position_set, self._position_id
        saved_rng = copy.deepcopy(self._np_random)
        saved_rng_seed = self._np_random_seed
        try:
            super().reset(seed=seed)
            position, dataset, position_id = curriculum.resolve_start(
                self.native, options, self.np_random, self._position_set
            )
            native_seed = int(self.np_random.integers(0, 2**64, dtype=np.uint64)) if seed is None else seed
            self.native = self.native.clone()
            self.native.reset(native_seed, position)
            self._position_set, self._position_id = dataset, position_id
            self._opponents()
        except Exception:
            self.native = saved_native
            self._position_set, self._position_id = saved_curriculum
            self._np_random = saved_rng
            self._np_random_seed = saved_rng_seed
            raise
        self._position_set, self._position_id = dataset, position_id
        self._needs_reset = any(self.native.flags())
        return self._frame(self.seat)

    def step(self, action: int) -> tuple[np.ndarray, float, bool, bool, dict[str, Any]]:
        if self._needs_reset:
            raise ResetNeeded("Call reset before stepping this episode")
        if isinstance(action, (bool, np.bool_)) or not isinstance(action, (int, np.integer)):
            raise ValueError("action must be an integer index")
        # Roll back the entire learner/opponent exchange if a custom policy fails.
        saved = self.native.clone()
        random_state = copy.deepcopy(self.np_random.bit_generator.state)
        try:
            rewards, _, _ = self.native.step(self.seat, int(action))
            reward = rewards[self.seat] + self._opponents()
        except Exception:
            self.native = saved
            self.np_random.bit_generator.state = random_state
            raise
        terminated, truncated = self.native.flags()
        self._needs_reset = terminated or truncated
        observation, info = self._frame(self.seat)
        return observation, float(reward), terminated, truncated, info

    def action_masks(self) -> np.ndarray:
        """Boolean legal-action mask used by MaskablePPO and other trainers."""
        if self._needs_reset:
            raise ResetNeeded("Call reset before requesting an active policy mask")
        return self.native.action_mask(self.seat)

    def render(self) -> str:
        return self.native.frame(self.seat)["text"]

    def close(self) -> None:
        pass

    def clone(self) -> GameEnv:
        """Clone engine and RNG state; an external policy callable is shared."""
        result = copy.copy(self)
        result.native = self.native.clone()
        result._np_random = copy.deepcopy(self.np_random)
        result._np_random_seed = self.np_random_seed
        result.action_space = copy.deepcopy(self.action_space)
        result.observation_space = copy.deepcopy(self.observation_space)
        return result

    def _opponent_identity(self):
        if self._stockfish is not None:
            return {"kind": "stockfish", "level": self._stockfish_level, **self._stockfish.identity()}
        return self.opponent if isinstance(self.opponent, str) else "python"

    def get_state(self) -> dict[str, Any]:
        """Trusted checkpoint containing private engine state and environment RNG."""
        return {
            "version": 3,
            "opponent": self._opponent_identity(),
            "seat": self.seat,
            "native": json.loads(self.native.get_state()),
            "rng": copy.deepcopy(self.np_random.bit_generator.state),
            "rng_seed": self.np_random_seed,
            "needs_reset": self._needs_reset,
            "curriculum": curriculum.checkpoint(self._position_set, self._position_id),
        }

    def set_state(self, state: dict[str, Any]) -> None:
        """Atomically restore this wrapper; external policy state is not included."""
        if not isinstance(state, dict) or type(state.get("version")) is not int:
            raise ValueError("incompatible environment checkpoint")
        version = state["version"]
        required = {"version", "seat", "native", "rng", "rng_seed", "needs_reset"}
        if version in (2, 3):
            required.add("curriculum")
        if version == 3:
            required.add("opponent")
        if (
            set(state) != required or version not in (1, 2, 3)
            or type(state["seat"]) is not int or state["seat"] != self.seat
            or type(state["rng_seed"]) is not int or state["rng_seed"] < -1
        ):
            raise ValueError("incompatible environment checkpoint")
        identity = self._opponent_identity()
        # Old checkpoints did not bind an opponent; their original default was random.
        if (version == 3 and state["opponent"] != identity) or (
            version < 3 and identity != "random"
        ):
            raise ValueError("checkpoint opponent differs from this environment")
        if not isinstance(state["needs_reset"], bool):
            raise ValueError("invalid checkpoint episode flag")
        candidate = self.native.clone()
        candidate.set_state(json.dumps(state["native"]))
        if any(candidate.flags()) and not state["needs_reset"]:
            raise ValueError("finished checkpoint cannot be marked active")
        dataset, position_id = curriculum.restore(state.get("curriculum"), candidate)
        generator = np.random.default_rng()
        generator.bit_generator.state = copy.deepcopy(state["rng"])
        self.native = candidate
        self._np_random = generator
        self._np_random_seed = state["rng_seed"]
        self._needs_reset = state["needs_reset"]
        self._position_set, self._position_id = dataset, position_id


def make(game: str, **kwargs: Any) -> GameEnv:
    """Create a Gymnasium environment for one model's seat."""
    return GameEnv(game, **kwargs)
