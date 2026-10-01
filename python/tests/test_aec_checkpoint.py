import copy
import json

import numpy as np
import pytest

from gamesforai import aec


def assert_same(left, right):
    assert left.get_state() == right.get_state()
    assert left.infos == right.infos
    if left.agents:
        assert left.agent_selection == right.agent_selection
        first, second = left.last(), right.last()
        np.testing.assert_array_equal(first[0]["observation"], second[0]["observation"])
        np.testing.assert_array_equal(first[0]["action_mask"], second[0]["action_mask"])
        assert first[1:] == second[1:]


def restore(env, game="tictactoe", config=None):
    other = aec(game, config=config)
    other.set_state(json.loads(json.dumps(env.get_state())))
    assert_same(env, other)
    return other


def test_checkpoint_before_reset_and_after_every_live_and_dead_turn():
    env = aec("tictactoe")
    other = restore(env)
    with pytest.raises(ValueError, match="reset"):
        other.step(0)
    env.reset(seed=42)
    for action in [0, 3, 1, 4, 2]:
        other = restore(env)
        env.step(action)
        other.step(action)
        assert_same(env, other)
    observed = {}
    while env.agents:
        other = restore(env)
        observed[env.agent_selection] = env.last()[1]
        env.step(None)
        other.step(None)
        assert_same(env, other)
    assert observed == {"player_0": 1.0, "player_1": -1.0}
    restore(env)


def test_truncated_episode_preserves_final_delivery():
    env = aec("chess", config={"max_plies": 2})
    env.reset(seed=5)
    for _ in range(2):
        env.step(int(np.flatnonzero(env.last()[0]["action_mask"])[0]))
    other = restore(env, "chess", {"max_plies": 2})
    while env.agents:
        assert env.last()[2:4] == (False, True)
        env.step(None)
        other.step(None)
        assert_same(env, other)


def test_curriculum_rng_and_independent_mutable_checkpoint():
    env = aec("connect4")
    env.reset(seed=19, options={"position_set": "connect4-solved-positions@1"})
    other = restore(env, "connect4")
    for _ in range(20):
        env.reset()
        other.reset()
        assert_same(env, other)
    state = env.get_state()
    state["agents"].clear()
    state["rewards"].clear()
    state["rng"].clear()
    assert env.agents and env.rewards and env.get_state()["rng"]


@pytest.mark.parametrize("field,value", [
    ("version", True),
    ("agents", ["player_0", "player_0"]),
    ("agents", ["player_1"]),
    ("agent_selection", "player_1"),
    ("skip_agent_selection", "player_1"),
    ("rewards", {"player_0": float("nan"), "player_1": 0.0}),
    ("cumulative_rewards", {}),
    ("terminations", {"player_0": True, "player_1": True}),
    ("truncations", {"player_0": 0, "player_1": 0}),
    ("rng", {}),
    ("initialized", False),
])
def test_invalid_restore_is_atomic(field, value):
    env = aec("tictactoe")
    env.reset(seed=7)
    before = env.get_state()
    invalid = copy.deepcopy(before)
    invalid[field] = value
    with pytest.raises((ValueError, TypeError)):
        env.set_state(invalid)
    assert env.get_state() == before


def test_engine_mismatch_and_curriculum_tampering_are_atomic():
    env = aec("connect4")
    env.reset(seed=8, options={"position_set": "connect4-solved-positions@1"})
    before = env.get_state()
    bad = copy.deepcopy(before)
    bad["curriculum"]["data"] += " "
    with pytest.raises(ValueError, match="hash"):
        env.set_state(bad)
    assert env.get_state() == before
    wrong = aec("tictactoe")
    wrong.reset(seed=8)
    with pytest.raises(ValueError):
        env.set_state(wrong.get_state())
    assert env.get_state() == before
