"""Reproducible local round robins between frozen model snapshots."""
from __future__ import annotations

import hashlib
import itertools
import json
import math
import re
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Callable

import numpy as np

from ._native import NativeEnv
from .positions import PositionSet
from .ratings import Rating
from .stockfish import StockfishPool
from .trajectories import EpisodeRecorder, _time


@dataclass(frozen=True)
class Agent:
    """One immutable snapshot identity and a fresh per-game policy factory.

    The factory receives (seed, seat) and returns a policy(observation, info).
    Alternatively opponent selects random or native minimax/mcts levels.
    """

    id: str
    policy_factory: Callable | None = None
    opponent: str | None = None
    initial_rating: Rating | None = None
    fixed: bool = False
    stockfish_pool: StockfishPool | None = None

    def __post_init__(self):
        if not isinstance(self.id, str) or not self.id or len(self.id.encode("utf-8")) > 256:
            raise ValueError("agent id must be nonempty text within 256 bytes")
        if type(self.fixed) is not bool or (self.fixed and self.initial_rating is None):
            raise ValueError("fixed anchors require an explicit calibrated initial_rating")
        if self.initial_rating is not None and not isinstance(self.initial_rating, Rating):
            raise ValueError("initial_rating must be a Rating")
        if self.initial_rating is not None and self.initial_rating.rd == 0 and not self.fixed:
            raise ValueError("zero rating deviation requires a fixed calibrated anchor")
        if (self.policy_factory is None) == (self.opponent is None):
            raise ValueError("choose a policy factory or built-in opponent")
        if self.policy_factory is not None and not callable(self.policy_factory):
            raise ValueError("policy_factory must be callable")
        if self.opponent is not None and (
            not isinstance(self.opponent, str)
            or (self.opponent != "random" and not re.fullmatch(r"(minimax|mcts|stockfish):([1-9]|10)", self.opponent))
        ):
            raise ValueError("opponent must be random, minimax:1..10, mcts:1..10, or stockfish:1..10")
        is_stockfish = self.opponent is not None and self.opponent.startswith("stockfish:")
        if self.stockfish_pool is not None and (not is_stockfish or not isinstance(self.stockfish_pool, StockfishPool)):
            raise ValueError("stockfish_pool is only valid for a Stockfish opponent")
        if is_stockfish and self.stockfish_pool is None:
            object.__setattr__(self, "stockfish_pool", StockfishPool())


def _wilson(wins, games):
    if games == 0:
        return None
    z = 1.959963984540054
    p = wins / games
    denominator = 1 + z * z / games
    center = (p + z * z / (2 * games)) / denominator
    radius = z * math.sqrt(p * (1 - p) / games + z * z / (4 * games * games)) / denominator
    return [0.0 if wins == 0 else max(0.0, center - radius),
            1.0 if wins == games else min(1.0, center + radius)]


def run_round_robin(
    game, agents, *, games_per_pair=2, seed=42, config=None, position_set=None,
    tau=0.5, run_id=None, created_at=None, on_episode=None, matchups=None,
):
    """Run balanced seats and rate completed games as one Glicko-2 period.

    Truncated, failed and already-finished starts are reported but never rated.
    Optional on_episode(match_summary, records) receives completed trajectories.
    A sink error aborts the run so a storage failure cannot silently lose data.
    """
    agents = tuple(agents)
    if not 2 <= len(agents) <= 32 or any(not isinstance(agent, Agent) for agent in agents):
        raise ValueError("a tournament requires 2..32 Agent snapshots")
    if len({agent.id for agent in agents}) != len(agents):
        raise ValueError("agent snapshot ids must be unique")
    if type(games_per_pair) is not int or not 2 <= games_per_pair <= 1000 or games_per_pair % 2:
        raise ValueError("games_per_pair must be even, 2..1000, for balanced seats")
    schedule = list(itertools.combinations(agents, 2))
    if matchups is not None:
        lookup, seen, schedule = {agent.id: agent for agent in agents}, set(), []
        for pair in matchups:
            if (
                not isinstance(pair, (list, tuple)) or len(pair) != 2
                or any(not isinstance(name, str) or name not in lookup for name in pair)
                or pair[0] == pair[1] or frozenset(pair) in seen
            ):
                raise ValueError("matchups must name unique pairs of distinct registered agents")
            seen.add(frozenset(pair))
            schedule.append((lookup[pair[0]], lookup[pair[1]]))
        if not schedule:
            raise ValueError("matchups must include at least one pair")
    total = len(schedule) * games_per_pair
    if total > 10000 or type(seed) is not int or not 0 <= seed <= 2**64 - total:
        raise ValueError("tournament exceeds 10000 games or the 64-bit seed range")
    if isinstance(tau, bool) or not isinstance(tau, (int, float)) or not 0.2 <= tau <= 1.2:
        raise ValueError("tau must be 0.2..1.2")
    prototype = NativeEnv(game, json.dumps(config or {}), seed)
    spec = json.loads(prototype.spec_json())
    if prototype.num_players != 2 or spec["turn_structure"] != "sequential":
        raise ValueError("round robin currently requires a sequential two-player game")
    for agent in agents:
        if agent.stockfish_pool is not None and game != "chess":
            raise ValueError("Stockfish matchups require chess")
        if agent.opponent not in (None, "random") and (
            spec["information"] != "perfect" or spec["stochastic"]
        ):
            raise ValueError("native search requires a deterministic perfect-information game")
    dataset = PositionSet.load(position_set) if isinstance(position_set, str) else position_set
    if dataset is not None:
        if not isinstance(dataset, PositionSet):
            raise ValueError("position_set must be a pinned name or PositionSet")
        dataset.validate_for(prototype)
    run_id = run_id or str(uuid.uuid4())
    if not isinstance(run_id, str) or not run_id or len(run_id.encode("utf-8")) > 200:
        raise ValueError("run_id must be nonempty text within 200 bytes")
    created_at = created_at or datetime.now(timezone.utc).isoformat()
    _time(created_at)
    if on_episode is not None and not callable(on_episode):
        raise ValueError("on_episode must be callable")
    priors = {agent.id: agent.initial_rating or Rating() for agent in agents}
    periods = {agent.id: [] for agent in agents}
    totals = {agent.id: {"wins": 0, "draws": 0, "losses": 0, "truncated": 0, "failed": 0,
                        "invalid_starts": 0, "seats": [0, 0]} for agent in agents}
    matches = []
    for left, right in schedule:
        for repetition in range(games_per_pair):
            order = (left, right) if repetition % 2 == 0 else (right, left)
            match_seed = seed + len(matches)
            rng = np.random.default_rng(match_seed)
            entry = None if dataset is None else dataset.sample(rng)
            episode = EpisodeRecorder(
                game, [agent.id for agent in order], config=config, seed=match_seed,
                position=None if entry is None else entry.position,
                episode_id=f"{run_id}:{len(matches)}", created_at=created_at,
                ratings=[priors[agent.id].rating for agent in order],
            )
            match = {
                "id": f"{run_id}:{len(matches)}", "seed": str(match_seed),
                "agents": [agent.id for agent in order], "turns": 0,
                "status": "completed", "failure": None, "returns": None,
                "position_id": None if entry is None else entry.id,
            }
            for seat, agent in enumerate(order):
                totals[agent.id]["seats"][seat] += 1
            policies = [None, None]
            if any(episode.flags):
                match["status"] = "invalid_start"
            else:
                for seat, agent in enumerate(order):
                    if agent.policy_factory is not None:
                        try:
                            policies[seat] = agent.policy_factory(match_seed, seat)
                            if not callable(policies[seat]):
                                raise TypeError("factory did not return a policy")
                        except Exception:
                            match["status"], match["failure"] = "failed", {"agent": agent.id, "code": "policy_factory"}
                            break
            while match["status"] == "completed" and episode.current_players:
                seat = episode.current_players[0]
                agent = order[seat]
                obs, info = episode.observation(seat)
                try:
                    if agent.opponent == "random":
                        action = int(rng.choice(np.flatnonzero(info["action_mask"])))
                    elif agent.opponent is not None:
                        algorithm, level = agent.opponent.split(":")
                        planning_seed = int(rng.integers(0, 2**64, dtype=np.uint64))
                        if algorithm == "stockfish":
                            action = episode.stockfish_action(agent.stockfish_pool, seat, int(level), planning_seed)
                        else:
                            action = episode.builtin_action(seat, algorithm, int(level), planning_seed)
                    else:
                        action = policies[seat](obs, info)
                except Exception:
                    match["status"], match["failure"] = "failed", {"agent": agent.id, "code": "policy_error"}
                    break
                if (
                    isinstance(action, (bool, np.bool_)) or not isinstance(action, (int, np.integer))
                    or not 0 <= action < len(info["action_mask"]) or not info["action_mask"][action]
                ):
                    match["status"], match["failure"] = "failed", {"agent": agent.id, "code": "illegal_action"}
                    break
                try:
                    episode.step(seat, int(action))
                except Exception:
                    match["status"], match["failure"] = "failed", {"agent": agent.id, "code": "episode_error"}
                    break
                match["turns"] += 1
            if match["status"] == "completed":
                match["returns"] = episode.returns
                if episode.flags[1]:
                    match["status"] = "truncated"
                else:
                    first, second = episode.returns
                    score = 1.0 if first > second else 0.0 if first < second else 0.5
                    for seat, value in enumerate((score, 1.0 - score)):
                        agent, opponent = order[seat], order[1 - seat]
                        periods[agent.id].append((priors[opponent.id], value))
                        totals[agent.id]["wins" if value == 1 else "losses" if value == 0 else "draws"] += 1
                rows = episode.records()
                encoded = json.dumps(rows, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
                match["trajectory_sha256"] = hashlib.sha256(encoded).hexdigest()
                if on_episode is not None:
                    on_episode(dict(match), rows)
            for agent in order:
                if match["status"] != "completed":
                    field = "invalid_starts" if match["status"] == "invalid_start" else match["status"]
                    totals[agent.id][field] += 1
            matches.append(match)

    standings = []
    for agent in agents:
        prior, results, stats = priors[agent.id], periods[agent.id], totals[agent.id]
        # No eligible games is an unchanged tournament, not an implicit inactive time period.
        if not results:
            updated = prior
        elif agent.fixed:
            updated = Rating(**{**prior.to_dict(), "games_played": prior.games_played + len(results)})
        else:
            updated = prior.update(results, tau=tau)
        games = len(results)
        standings.append({
            "agent": agent.id, **stats, "rated_games": games, "fixed": agent.fixed,
            "initial_rating": prior.to_dict(), "rating": updated.to_dict(),
            "rating_interval95": list(updated.interval95),
            "win_rate": None if games == 0 else stats["wins"] / games,
            "win_rate_wilson95": _wilson(stats["wins"], games),
            "score": None if games == 0 else (stats["wins"] + 0.5 * stats["draws"]) / games,
        })
    standings.sort(key=lambda row: (-row["rating"]["rating"], row["agent"]))
    return {
        "format_version": 1, "run_id": run_id, "game": game,
        "engine_version": spec["engine_version"],
        "config": json.loads(prototype.get_state())["config"],
        "seed": str(seed), "created_at": created_at, "games_per_pair": games_per_pair,
        "rating_system": {"name": "glicko2", "tau": tau, "period": "whole_tournament"},
        "position_set": None if dataset is None else {"id": dataset.identifier, "sha256": dataset.sha256},
        "agent_specs": [{
            "id": agent.id, "opponent": agent.opponent,
            "stockfish": None if agent.stockfish_pool is None else agent.stockfish_pool.identity(),
        } for agent in agents],
        "matchups": [[left.id, right.id] for left, right in schedule],
        "standings": standings, "matches": matches,
    }
