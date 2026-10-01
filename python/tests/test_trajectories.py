import json

import numpy as np
import pytest

from gamesforai.trajectories import EpisodeRecorder, select_records, write_jsonl


def episode(**kwargs):
    env = EpisodeRecorder("tictactoe", ["my-model", "other-model"], seed=42,
                          ratings=[1600, None], episode_id="example",
                          created_at="2026-01-01T12:00:00+00:00", **kwargs)
    for action in [0, 3, 1, 4, 2]:
        seat = env.current_players[0]
        obs, info = env.observation(seat)
        assert obs.dtype == np.float32
        assert info["action_mask"][action]
        env.step(seat, action, reasoning="legal move", transcript={"messages": ["test fixture"]})
    return env


def test_recorded_transitions_masks_outcomes_and_independent_arrays(tmp_path):
    env = episode()
    rows = env.records()
    assert len(rows) == 5
    assert [row["outcome"] for row in rows] == ["win", "loss", "win", "loss", "win"]
    assert rows[-1]["rewards"] == [1.0, -1.0]
    assert all(row["episode_returns"] == [1.0, -1.0] for row in rows)
    assert rows[-1]["terminated"] and not rows[-1]["truncated"]
    for row in rows:
        assert row["action_mask"][row["action_index"]]
        assert row["observation_text"] and row["action"]
        assert len(row["observation"]) == np.prod(row["observation_shape"])
        assert set(row).isdisjoint({"state", "native", "expected", "solution", "rng"})
        assert json.loads(row["transcript_json"]) == {"messages": ["test fixture"]}
    path = tmp_path / "episode.jsonl"
    assert write_jsonl(path, rows) == 5
    loaded = [json.loads(line) for line in path.read_text().splitlines()]
    assert loaded == rows
    rows[0]["observation"][0] = 999
    assert env.records()[0]["observation"][0] != 999
    assert episode().records() == env.records()


def test_filters_game_agent_rating_date_and_outcome():
    rows = episode().records()
    filtered = list(select_records(rows, game="tictactoe", agent="my-model",
                                   min_rating=1500, max_rating=1700, outcome="win",
                                   after="2026-01-01T00:00:00Z", before="2026-01-02T00:00:00Z"))
    assert len(filtered) == 3
    assert not list(select_records(rows, max_rating=1500))
    assert not list(select_records(rows, game="chess"))
    assert not list(select_records(rows, before="2026-01-01T12:00:00Z"))
    with pytest.raises(ValueError, match="timezone"):
        list(select_records(rows, after="2026-01-01"))
    with pytest.raises(ValueError):
        list(select_records(rows, min_rating=1700, max_rating=1500))


def test_failed_moves_and_record_limits_preserve_native_state():
    env = EpisodeRecorder("connect4", ["one", "two"], max_bytes=1)
    before, info = env.observation(0)
    with pytest.raises(ValueError, match="byte limit"):
        env.step(0, 3)
    after, after_info = env.observation(0)
    np.testing.assert_array_equal(before, after)
    np.testing.assert_array_equal(info["action_mask"], after_info["action_mask"])
    assert env.current_players == [0]
    with pytest.raises(ValueError, match="legal"):
        env.step(0, 100)
    with pytest.raises(ValueError, match="finish"):
        env.records()


def test_truncated_labels_never_become_draws():
    env = EpisodeRecorder("chess", ["one", "two"], config={"max_plies": 2})
    for _ in range(2):
        seat = env.current_players[0]
        env.step(seat, int(np.flatnonzero(env.observation(seat)[1]["action_mask"])[0]))
    assert all(row["outcome"] == "truncated" for row in env.records())
    assert env.records()[-1]["truncated"]


def test_failed_export_keeps_previous_file_and_cleans_temporary(tmp_path):
    path = tmp_path / "train.jsonl"
    path.write_text("previous", encoding="utf-8")

    def broken():
        yield episode().records()[0]
        raise RuntimeError("failed collection")

    with pytest.raises(RuntimeError):
        write_jsonl(path, broken())
    assert path.read_text() == "previous"
    assert list(tmp_path.iterdir()) == [path]
