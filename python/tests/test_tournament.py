import json

import numpy as np
import pytest

from gamesforai.ratings import Rating
from gamesforai.tournament import Agent, run_round_robin


def run(agents=None, **kwargs):
    return run_round_robin("tictactoe", agents or [
        Agent("search-v1", opponent="minimax:3"),
        Agent("random-v1", opponent="random"),
        Agent("random-v2", opponent="random"),
    ], seed=19, run_id="test-league", created_at="2026-01-01T00:00:00Z", **kwargs)


def test_round_robin_balances_seats_repeats_and_exports_completed_games():
    saved = []
    first = run(games_per_pair=4, on_episode=lambda summary, rows: saved.append((summary, rows)))
    second = run(games_per_pair=4)
    assert first == second
    assert len(first["matches"]) == len(saved) == 12
    assert all(row["seats"] == [4, 4] and row["rated_games"] == 8 for row in first["standings"])
    assert all(row["rating"]["games_played"] == 8 for row in first["standings"])
    assert sum(row["wins"] for row in first["standings"]) == sum(row["losses"] for row in first["standings"])
    for summary, rows in saved:
        assert summary["status"] == "completed"
        assert rows[-1]["terminated"]
        assert all(row["episode_id"] == summary["id"] for row in rows)
    json.dumps(first, allow_nan=False)


def test_python_factories_reset_each_game_and_only_receive_policy_inputs():
    calls = []

    def factory(seed, seat):
        calls.append((seed, seat))
        rng = np.random.default_rng(seed + seat)

        def choose(obs, info):
            assert "native" not in info and "state" not in info
            return int(rng.choice(np.flatnonzero(info["action_mask"])))
        return choose

    report = run([Agent("my-model", policy_factory=factory), Agent("random", opponent="random")], games_per_pair=4)
    assert calls == [(19, 0), (20, 1), (21, 0), (22, 1)]
    assert all(match["status"] == "completed" for match in report["matches"])


def test_failures_and_truncations_are_not_wins_draws_or_rating_updates():
    invalid = Agent("bad-policy", policy_factory=lambda seed, seat: lambda obs, info: 999)
    report = run([invalid, Agent("random", opponent="random")])
    assert all(match["status"] == "failed" for match in report["matches"])
    assert all(row["rated_games"] == 0 and row["rating"] == Rating().to_dict() for row in report["standings"])
    report = run_round_robin("chess", [Agent("one", opponent="random"), Agent("two", opponent="random")],
                             config={"max_plies": 2})
    assert all(match["status"] == "truncated" for match in report["matches"])
    assert all(row["truncated"] == 2 and row["rated_games"] == 0 for row in report["standings"])


def test_fixed_anchor_and_reported_win_rate_interval():
    report = run([Agent("anchor", opponent="random", initial_rating=Rating(rating=1800, rd=0), fixed=True),
                  Agent("learner", opponent="minimax:3")], games_per_pair=4)
    anchor = next(row for row in report["standings"] if row["agent"] == "anchor")
    assert anchor["rating"]["rating"] == 1800 and anchor["rating"]["rd"] == 0
    assert anchor["rating"]["games_played"] == 4
    for row in report["standings"]:
        low, high = row["win_rate_wilson95"]
        assert low <= row["win_rate"] <= high


def test_policy_error_details_are_not_exported_and_sink_failures_propagate():
    def fail(seed, seat):
        raise RuntimeError("private-model-key")
    report = run([Agent("failure", policy_factory=fail), Agent("random", opponent="random")])
    assert "private-model-key" not in json.dumps(report)

    def sink(summary, rows):
        raise RuntimeError("disk full")
    with pytest.raises(RuntimeError, match="disk full"):
        run(on_episode=sink)


def test_invalid_schedules_and_agent_configs_are_rejected():
    with pytest.raises(ValueError):
        run(games_per_pair=3)
    with pytest.raises(ValueError):
        run([Agent("same", opponent="random"), Agent("same", opponent="random")])
    with pytest.raises(ValueError):
        Agent("anchor", opponent="random", fixed=True)
    with pytest.raises(ValueError):
        Agent("missing")
    with pytest.raises(ValueError):
        run_round_robin("sudoku", [Agent("one", opponent="random"), Agent("two", opponent="random")])
