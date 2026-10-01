import copy
import json

import numpy as np
import pytest

from gamesforai import NativeEnv, PositionSet, VectorEnv


def test_seeded_position_batches_match_individual_imports_and_threads():
    dataset = PositionSet.load("connect4-solved-positions@1")
    entries = {entry.id: entry for entry in dataset.entries}
    serial = VectorEnv("connect4", 16, threads=1)
    parallel = VectorEnv("connect4", 16, threads=4)
    left = serial.reset(41, options={"position_set": dataset})
    right = parallel.reset(41, options={"position_set": dataset.identifier})
    assert serial.get_state() == parallel.get_state()
    assert serial.origins == parallel.origins
    for key in left:
        np.testing.assert_array_equal(left[key], right[key])
    for index, origin in enumerate(serial.origins):
        entry = entries[origin["curriculum"]["position_id"]]
        single = NativeEnv("connect4")
        single.reset(41 + index, entry.position)
        np.testing.assert_array_equal(left["observation"][index], single.frame(0)["observation"])
    assert "expected" not in json.dumps(serial.origins)


def test_partial_reset_preserves_other_rows_and_cloned_curricula():
    env = VectorEnv("connect4", 4)
    env.reset(3, options={"position_set": "connect4-solved-positions@1"})
    before, origins = env.get_state(), env.origins
    clone = env.clone()
    clone.reset_at([1], [91], options={"position_set": None})
    assert clone.origins[1] == {}
    assert env.origins == origins
    for index in [0, 2, 3]:
        assert clone.get_state()["native"][index] == before["native"][index]
        assert clone.origins[index] == origins[index]
    clone.reset_at([1], [95])
    assert clone.origins[1] == {}
    assert len(env.get_state()["datasets"]) == 1


def test_restore_repeats_curriculum_resets_and_legacy_clears_them():
    env = VectorEnv("connect4", 8)
    legacy = env.get_state()
    env.reset(82, options={"position_set": "connect4-solved-positions@1"})
    snapshot = json.loads(json.dumps(env.get_state()))
    other = VectorEnv("connect4", 8)
    other.set_state(snapshot)
    for seed in range(6):
        env.reset(seed)
        other.reset(seed)
        assert env.get_state() == other.get_state()
    other.set_state(legacy)
    assert other.get_state() == legacy
    assert other.origins == [{}] * 8
    assert isinstance(other.get_state(), list)


def test_failed_reset_and_restore_leave_native_and_curricula_unchanged():
    env = VectorEnv("connect4", 4)
    env.reset(31, options={"position_set": "connect4-solved-positions@1"})
    before = env.get_state()
    for kwargs in [
        {"indices": [0, 0], "seeds": [1, 2]},
        {"indices": [0], "seeds": [True]},
        {"indices": [0], "seeds": [2**64]},
        {"indices": [0], "seeds": [1], "options": {"position_set": "chess-endgames-basic@1"}},
        {"indices": [0, 1], "seeds": [1, 2], "positions": [None, "bad"]},
    ]:
        with pytest.raises(ValueError):
            env.reset_at(**kwargs)
        assert env.get_state() == before
    bad = copy.deepcopy(before)
    bad["rows"][0]["position_id"] = "missing"
    with pytest.raises(ValueError):
        env.set_state(bad)
    assert env.get_state() == before
    bad = copy.deepcopy(before)
    bad["native"][2] = "{}"
    with pytest.raises(ValueError):
        env.set_state(bad)
    assert env.get_state() == before
