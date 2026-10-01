"""PettingZoo AEC interface for independent policies and self-play."""
from __future__ import annotations

import copy
import json
import math
from typing import Any

import numpy as np
from gymnasium import spaces
from gymnasium.utils import seeding
from pettingzoo import AECEnv

from ._native import NativeEnv
from . import curriculum


class AECGameEnv(AECEnv):
    """Sequential game turns with a separate information set for each agent."""

    metadata = {"render_modes": ["ansi"], "name": "gamesforai_v0", "is_parallelizable": False}

    def __init__(
        self, game: str, *, config: dict[str, Any] | None = None, render_mode: str | None = None
    ) -> None:
        if render_mode not in (None, "ansi"):
            raise ValueError("render_mode must be None or 'ansi'")
        self.native = NativeEnv(game, json.dumps(config or {}))
        self.render_mode = render_mode
        self.possible_agents = [f"player_{seat}" for seat in range(self.native.num_players)]
        self._seats = {agent: seat for seat, agent in enumerate(self.possible_agents)}
        self._actions = {agent: spaces.Discrete(self.native.action_space_size) for agent in self.possible_agents}
        self._observations = {}
        for agent, seat in self._seats.items():
            shape = self.native.frame(seat)["observation"].shape
            self._observations[agent] = spaces.Dict({
                "observation": spaces.Box(-np.inf, np.inf, shape=shape, dtype=np.float32),
                "action_mask": spaces.MultiBinary(self.native.action_space_size),
            })
        self.agents = []
        self.rewards = {}
        self._cumulative_rewards = {}
        self.terminations = {}
        self.truncations = {}
        self.infos = {}
        self.agent_selection = None
        self._skip_agent_selection = None
        self._initialized = False
        self._position_set = None
        self._position_id = None
        self._rng, _ = seeding.np_random(None)

    def observation_space(self, agent):
        return self._observations[agent]

    def action_space(self, agent):
        return self._actions[agent]

    def observe(self, agent):
        frame = self.native.frame(self._seats[agent])
        return {"observation": frame["observation"], "action_mask": frame["action_mask"]}

    def _refresh_infos(self):
        self.infos = {
            agent: {"seat": self._seats[agent], "text": self.native.frame(self._seats[agent])["text"],
                    **curriculum.origin_info(self._position_set, self._position_id)}
            for agent in self.agents
        }

    def reset(self, seed=None, options=None):
        options = options or {}
        # PettingZoo callers may pass options intended for other wrappers.
        rng = copy.deepcopy(self._rng) if seed is None else seeding.np_random(seed)[0]
        position, dataset, position_id = curriculum.resolve_start(
            self.native, options, rng, self._position_set
        )
        native_seed = int(rng.integers(0, 2**64, dtype=np.uint64)) if seed is None else seed
        candidate = self.native.clone()
        candidate.reset(native_seed, position)
        actors = candidate.current_players()
        if len(actors) > 1:
            raise ValueError("AEC adapter currently requires sequential turns")
        self.native, self._rng = candidate, rng
        self._position_set, self._position_id = dataset, position_id
        self.agents = self.possible_agents[:]
        self._initialized = True
        self.rewards = {agent: 0.0 for agent in self.agents}
        self._cumulative_rewards = self.rewards.copy()
        terminated, truncated = self.native.flags()
        self.terminations = {agent: terminated for agent in self.agents}
        self.truncations = {agent: truncated for agent in self.agents}
        self.agent_selection = self.possible_agents[actors[0]] if actors else self.agents[0]
        self._skip_agent_selection = None
        self._refresh_infos()

    def step(self, action):
        if not self.agents:
            raise ValueError("reset before stepping an empty AEC episode")
        agent = self.agent_selection
        if self.terminations[agent] or self.truncations[agent]:
            self._was_dead_step(action)
            return
        if isinstance(action, (bool, np.bool_)) or not isinstance(action, (int, np.integer)):
            raise ValueError("a live agent must choose an integer action index")
        # Do not change bookkeeping until the native engine accepts the action.
        rewards, terminated, truncated = self.native.step(self._seats[agent], int(action))
        self._cumulative_rewards[agent] = 0.0
        self.rewards = {name: float(rewards[self._seats[name]]) for name in self.agents}
        self.terminations = {name: terminated for name in self.agents}
        self.truncations = {name: truncated for name in self.agents}
        actors = self.native.current_players()
        if actors:
            self.agent_selection = self.possible_agents[actors[0]]
        self._refresh_infos()
        self._accumulate_rewards()
        self._deads_step_first()

    def render(self):
        return self.native.public_text()

    def close(self):
        pass

    def get_state(self) -> dict[str, Any]:
        """Private, JSON-compatible checkpoint including pending AEC rewards."""
        return {
            "version": 1,
            "native": json.loads(self.native.get_state()),
            "rng": copy.deepcopy(self._rng.bit_generator.state),
            "curriculum": curriculum.checkpoint(self._position_set, self._position_id),
            "initialized": self._initialized,
            "agents": self.agents[:],
            "agent_selection": self.agent_selection,
            "skip_agent_selection": self._skip_agent_selection,
            "rewards": self.rewards.copy(),
            "cumulative_rewards": self._cumulative_rewards.copy(),
            "terminations": self.terminations.copy(),
            "truncations": self.truncations.copy(),
        }

    def set_state(self, state: dict[str, Any]) -> None:
        """Validate a checkpoint completely before replacing any live state."""
        required = {
            "version", "native", "rng", "curriculum", "initialized", "agents",
            "agent_selection", "skip_agent_selection", "rewards",
            "cumulative_rewards", "terminations", "truncations",
        }
        if (
            not isinstance(state, dict) or set(state) != required
            or type(state["version"]) is not int or state["version"] != 1
            or type(state["initialized"]) is not bool
        ):
            raise ValueError("incompatible AEC checkpoint")
        agents = state["agents"]
        if (
            not isinstance(agents, list)
            or any(not isinstance(agent, str) or agent not in self._seats for agent in agents)
            or agents != [agent for agent in self.possible_agents if agent in agents]
        ):
            raise ValueError("invalid AEC checkpoint agents")
        for field in ("rewards", "cumulative_rewards", "terminations", "truncations"):
            values = state[field]
            if not isinstance(values, dict) or set(values) != set(agents):
                raise ValueError("invalid AEC checkpoint bookkeeping")
            if field in ("terminations", "truncations"):
                if any(type(value) is not bool for value in values.values()):
                    raise ValueError("invalid AEC checkpoint flags")
            elif any(
                type(value) not in (int, float) or not math.isfinite(value)
                for value in values.values()
            ):
                raise ValueError("invalid AEC checkpoint rewards")
        selection, skip = state["agent_selection"], state["skip_agent_selection"]
        if any(value is not None and value not in self.possible_agents for value in (selection, skip)):
            raise ValueError("invalid AEC checkpoint turn")
        candidate = self.native.clone()
        candidate.set_state(json.dumps(state["native"]))
        terminated, truncated = candidate.flags()
        actors = candidate.current_players()
        if not state["initialized"]:
            if agents or selection is not None or skip is not None:
                raise ValueError("uninitialized AEC checkpoint has live bookkeeping")
        elif not terminated and not truncated:
            if (
                agents != self.possible_agents or len(actors) != 1
                or selection != self.possible_agents[actors[0]] or skip is not None
            ):
                raise ValueError("AEC checkpoint turn does not match engine")
        elif (agents and selection != agents[0]) or (not agents and skip is not None):
            raise ValueError("invalid AEC checkpoint dead-agent order")
        if any(value != terminated for value in state["terminations"].values()) or any(
            value != truncated for value in state["truncations"].values()
        ):
            raise ValueError("AEC checkpoint flags do not match engine")
        generator = np.random.default_rng()
        generator.bit_generator.state = copy.deepcopy(state["rng"])
        dataset, position_id = curriculum.restore(state["curriculum"], candidate)

        # Derive information sets from the validated engine, never serialized infos.
        restored = copy.copy(self)
        restored.native = candidate
        restored._rng = generator
        restored._position_set, restored._position_id = dataset, position_id
        restored._initialized = state["initialized"]
        restored.agents = agents[:]
        restored.agent_selection, restored._skip_agent_selection = selection, skip
        restored.rewards = state["rewards"].copy()
        restored._cumulative_rewards = state["cumulative_rewards"].copy()
        restored.terminations = state["terminations"].copy()
        restored.truncations = state["truncations"].copy()
        restored._refresh_infos()
        self.__dict__.update(restored.__dict__)

    def clone(self):
        """Independent native and AEC bookkeeping state for trusted local search."""
        result = copy.copy(self)
        result.native = self.native.clone()
        for name in (
            "agents", "rewards", "_cumulative_rewards", "terminations", "truncations",
            "infos", "_actions", "_observations", "_rng",
        ):
            if hasattr(self, name):
                setattr(result, name, copy.deepcopy(getattr(self, name)))
        return result


def aec(game: str, **kwargs: Any) -> AECGameEnv:
    """Create a PettingZoo AEC environment with one policy per game seat."""
    return AECGameEnv(game, **kwargs)
