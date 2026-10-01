import copy
import json

import numpy as np
import pytest

from gamesforai import make


@pytest.mark.parametrize("opponent", ["random", "minimax:1", "minimax:3", "mcts:2"])
def test_resume_repeats_policy_decisions_and_rng(opponent):
    env = make("connect4", opponent=opponent)
    env.reset(seed=87)
    restored = make("connect4", opponent=opponent)
    restored.set_state(json.loads(json.dumps(env.get_state())))
    for _ in range(4):
        action = int(np.flatnonzero(env.action_masks())[0])
        first, second = env.step(action), restored.step(action)
        np.testing.assert_array_equal(first[0], second[0])
        assert first[1:4] == second[1:4]
        assert env.get_state() == restored.get_state()
        if first[2] or first[3]:
            break


@pytest.mark.parametrize("opponent", ["random", "minimax:2", "mcts:3"])
def test_wrong_opponent_is_rejected_without_state_changes(opponent):
    original = make("connect4", opponent="minimax:3")
    original.reset(seed=3)
    other = make("connect4", opponent=opponent)
    other.reset(seed=5)
    before = other.get_state()
    with pytest.raises(ValueError, match="opponent"):
        other.set_state(original.get_state())
    assert other.get_state() == before


def test_legacy_checkpoints_bind_only_to_random_opponent():
    random = make("connect4")
    random.reset(seed=81)
    legacy = random.get_state()
    del legacy["opponent"]
    for version in (2, 1):
        legacy["version"] = version
        if version == 1:
            del legacy["curriculum"]
        random.set_state(legacy)
        search = make("connect4", opponent="minimax:1")
        with pytest.raises(ValueError, match="opponent"):
            search.set_state(legacy)


@pytest.mark.parametrize("field,value", [
    ("version", True), ("seat", False), ("rng_seed", True), ("rng_seed", -2),
])
def test_malformed_metadata_is_atomic(field, value):
    env = make("connect4")
    env.reset(seed=4)
    before = env.get_state()
    invalid = copy.deepcopy(before)
    invalid[field] = value
    with pytest.raises(ValueError):
        env.set_state(invalid)
    assert env.get_state() == before


def test_python_policy_state_remains_explicitly_caller_owned():
    def policy(obs, info):
        return int(np.flatnonzero(info["action_mask"])[0])

    env = make("connect4", opponent=policy)
    env.reset(seed=2)
    state = env.get_state()
    assert state["opponent"] == "python"
    restored = make("connect4", opponent=policy)
    restored.set_state(state)
    assert restored.get_state() == state
    with pytest.raises(ValueError, match="opponent"):
        make("connect4").set_state(state)
