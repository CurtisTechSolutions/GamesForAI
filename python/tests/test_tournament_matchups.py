import numpy as np
import pytest

import gamesforai.stockfish as stockfish_module
from gamesforai import StockfishPool
from gamesforai.tournament import Agent, _wilson, run_round_robin


def test_explicit_matchups_skip_unrequested_pairs_and_preserve_balance():
    agents = [Agent(name, opponent="random") for name in ["model", "engine-one", "engine-two"]]
    report = run_round_robin("tictactoe", agents, games_per_pair=4,
                             matchups=[("model", "engine-one"), ("model", "engine-two")])
    assert len(report["matches"]) == 8
    assert all("model" in match["agents"] for match in report["matches"])
    stats = {row["agent"]: row for row in report["standings"]}
    assert stats["model"]["seats"] == [4, 4]
    assert stats["engine-one"]["seats"] == stats["engine-two"]["seats"] == [2, 2]
    for invalid in [[], [("model", "missing")], [("model", "model")],
                    [("model", "engine-one"), ("engine-one", "model")]]:
        with pytest.raises(ValueError, match="matchups"):
            run_round_robin("tictactoe", agents, matchups=invalid)


def test_stockfish_matchups_record_binary_identity_and_unrated_truncation(tmp_path, monkeypatch):
    class Fixture:
        def __init__(self, path, workers):
            pass

        def choose(self, env, seat, level, seed, time_ms):
            return int(np.flatnonzero(env.frame(seat)["action_mask"])[0])

    monkeypatch.setattr(stockfish_module, "NativeUciPool", Fixture)
    path = tmp_path / "engine"
    path.write_bytes(b"immutable engine fixture")
    pool = StockfishPool(path)
    agents = [Agent("model", opponent="random"),
              Agent("engine", opponent="stockfish:5", stockfish_pool=pool)]
    report = run_round_robin("chess", agents, config={"max_plies": 4})
    assert all(match["status"] == "truncated" for match in report["matches"])
    assert all(row["rated_games"] == 0 for row in report["standings"])
    assert report["agent_specs"][1]["stockfish"] == pool.identity()
    with pytest.raises(ValueError, match="chess"):
        run_round_robin("connect4", agents)


def test_wilson_interval_exact_probability_endpoints():
    for games in [1, 2, 4, 10, 100, 200]:
        assert _wilson(0, games)[0] == 0
        assert _wilson(games, games)[1] == 1
