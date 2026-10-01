import numpy as np
import pytest
from pettingzoo.test import api_test

from gamesforai import aec


@pytest.mark.parametrize("game,config", [
    ("tictactoe", {}),
    ("connect4", {}),
    ("chess", {"max_plies": 12}),
    ("sudoku", {"size": 4, "max_moves": 8, "allow_notes": False}),
])
def test_pettingzoo_contract(game, config):
    api_test(aec(game, config=config), num_cycles=100)


def test_final_rewards_are_delivered_to_both_agents_before_removal():
    env = aec("tictactoe")
    env.reset(seed=0)
    for action in [0, 3, 1, 4, 2]:
        env.step(action)
    assert env.agents == ["player_0", "player_1"]
    rewards = {}
    for agent in env.agent_iter():
        observation, reward, terminated, truncated, info = env.last()
        assert terminated and not truncated
        assert not observation["action_mask"].any()
        rewards[agent] = reward
        env.step(None)
    assert env.agents == []
    assert rewards == {"player_0": 1.0, "player_1": -1.0}


def test_aec_clone_and_invalid_steps_preserve_turn_bookkeeping():
    env = aec("connect4")
    env.reset(seed=10)
    clone = env.clone()
    before = env.native.get_state()
    with pytest.raises(ValueError):
        env.step(99)
    assert env.native.get_state() == before
    assert env.agent_selection == "player_0"
    assert env.rewards == {"player_0": 0.0, "player_1": 0.0}
    env.step(3)
    assert clone.native.get_state() == before
    assert clone.agent_selection == "player_0"
    assert env.agent_selection == "player_1"
    assert not env.observe("player_0")["action_mask"].any()
    assert env.observe("player_1")["action_mask"].any()


def test_seeded_self_play_and_cap_endings():
    first = aec("chess", config={"max_plies": 8})
    second = aec("chess", config={"max_plies": 8})
    first.reset(seed=41)
    second.reset(seed=41)
    for _ in range(8):
        assert first.agent_selection == second.agent_selection
        observation, _, terminated, truncated, _ = first.last()
        assert not terminated and not truncated
        other = second.last()[0]
        np.testing.assert_array_equal(observation["observation"], other["observation"])
        action = int(np.flatnonzero(observation["action_mask"])[0])
        first.step(action)
        second.step(action)
    assert all(first.truncations.values())
    assert not any(first.terminations.values())
    assert first.native.get_state() == second.native.get_state()
    assert first.render() == first.native.public_text()
