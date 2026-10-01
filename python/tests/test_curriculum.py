import copy
import json

import numpy as np
import pytest

from gamesforai import PositionSet, aec, make


def test_gym_curriculum_repeats_seed_and_continues_rng_after_restore():
    env = make("connect4")
    first, info = env.reset(seed=82, options={"position_set": "connect4-solved-positions@1"})
    origin = info["curriculum"]
    assert set(origin) == {"position_set", "sha256", "position_id"}
    assert "expected" not in json.dumps(info, default=lambda array: array.tolist())
    second, second_info = env.reset(seed=82)
    np.testing.assert_array_equal(first, second)
    assert second_info["curriculum"] == origin
    snapshot = env.get_state()
    continuation = [env.reset()[1]["curriculum"]["position_id"] for _ in range(20)]
    env.set_state(snapshot)
    assert continuation == [env.reset()[1]["curriculum"]["position_id"] for _ in range(20)]
    env.set_state(snapshot)
    clone = env.clone()
    assert env.reset()[1]["curriculum"] == clone.reset()[1]["curriculum"]
    assert "curriculum" not in env.reset(options={"position_set": None})[1]


def test_curriculum_config_and_invalid_reset_preserve_checkpoint():
    env = make("connect4")
    env.reset(seed=4)
    before = env.get_state()
    for options in [
        {"position_set": "chess-endgames-basic@1"},
        {"position": "bad"},
        {"position": "bad", "position_set": "connect4-solved-positions@1"},
    ]:
        with pytest.raises(ValueError):
            env.reset(seed=999, options=options)
        assert env.get_state() == before
    env.reset(seed=4, options={"position_set": PositionSet.load("connect4-solved-positions@1")})
    before = env.get_state()
    broken = copy.deepcopy(before)
    broken["curriculum"]["data"] += " "
    with pytest.raises(ValueError, match="hash"):
        env.set_state(broken)
    assert env.get_state() == before
    with pytest.raises(ValueError, match="config"):
        make("chess", config={"max_plies": 8}).reset(
            seed=1, options={"position_set": "chess-endgames-basic@1"}
        )


def test_older_gym_checkpoints_remain_loadable():
    env = make("tictactoe")
    env.reset(seed=1)
    snapshot = env.get_state()
    snapshot["version"] = 1
    del snapshot["curriculum"]
    env.set_state(snapshot)
    assert env.get_state()["native"] == snapshot["native"]


def test_aec_curricula_clone_and_failed_reset_are_reproducible():
    env = aec("chess")
    env.reset(seed=91, options={"position_set": "chess-endgames-basic@1"})
    origin = env.infos[env.agent_selection]["curriculum"]
    clone = env.clone()
    seen = []
    for _ in range(20):
        env.reset()
        clone.reset()
        assert env.native.get_state() == clone.native.get_state()
        assert env.agent_selection == clone.agent_selection
        assert env.infos == clone.infos
        seen.append(env.infos[env.agent_selection]["curriculum"]["position_id"])
    assert len(set(seen)) > 1
    before = env.clone()
    with pytest.raises(ValueError):
        env.reset(seed=123, options={"position": "invalid FEN"})
    assert env.native.get_state() == before.native.get_state()
    env.reset()
    before.reset()
    assert env.native.get_state() == before.native.get_state()
    env.reset(seed=91)
    assert env.infos[env.agent_selection]["curriculum"] == origin
    assert "expected" not in json.dumps(env.infos)


def test_opponent_opening_sees_current_origin_and_failure_rolls_back():
    received = []
    fail = False

    def policy(obs, info):
        received.append(info.get("curriculum"))
        if fail:
            raise ValueError("opponent failure")
        return int(np.flatnonzero(info["action_mask"])[0])

    env = make("connect4", seat=1, opponent=policy)
    env.reset(seed=8, options={"position_set": "connect4-solved-positions@1"})
    assert received[-1]["position_set"] == "connect4-solved-positions@1"
    assert received[-1]["position_id"] == env.get_state()["curriculum"]["position_id"]
    before = env.get_state()
    fail = True
    with pytest.raises(ValueError, match="opponent failure"):
        env.reset(seed=999)
    assert env.get_state() == before
