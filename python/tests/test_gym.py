import copy
import numpy as np
import pytest
from gymnasium.error import ResetNeeded
from gymnasium.utils.env_checker import check_env

from gamesforai import make


@pytest.mark.parametrize("game", ["tictactoe", "connect4"])
def test_gymnasium_contract(game):
    check_env(make(game, render_mode="ansi"), skip_render_check=False)


@pytest.mark.parametrize("game,config", [
    ("tictactoe", {}),
    ("connect4", {}),
    ("chess", {"max_plies": 12}),
    ("sudoku", {"size": 4, "max_moves": 8, "allow_notes": False}),
])
def test_seeded_masked_episodes_and_shapes(game, config):
    first = make(game, config=config)
    second = make(game, config=config)
    obs, info = first.reset(seed=7)
    other, other_info = second.reset(seed=7)
    np.testing.assert_array_equal(obs, other)
    for _ in range(100):
        assert first.observation_space.contains(obs)
        assert info["action_mask"].dtype == np.int8
        action = int(np.flatnonzero(info["action_mask"])[0])
        left = first.step(action)
        right = second.step(action)
        np.testing.assert_array_equal(left[0], right[0])
        np.testing.assert_array_equal(left[4]["action_mask"], right[4]["action_mask"])
        assert left[1:4] == right[1:4]
        obs, _, terminated, truncated, info = left
        if terminated or truncated:
            break
    else:
        pytest.fail("bounded episode did not end")
    with pytest.raises(ResetNeeded):
        first.step(0)


def test_second_seat_receives_opponent_opening():
    env = make("tictactoe", seat=1)
    _, info = env.reset(seed=19)
    assert info["to_act"] == [1]
    assert info["action_mask"].sum() == 8


def test_python_policy_terminal_reward_and_clone_restore():
    def opponent(observation, info):
        assert info["seat"] == 1
        assert observation.dtype == np.float32
        return int(np.flatnonzero(info["action_mask"])[-1])

    env = make("tictactoe", opponent=opponent)
    env.reset(seed=9)
    saved = env.get_state()
    clone = env.clone()
    for action in [0, 1]:
        assert env.step(action)[1:4] == (0.0, False, False)
    assert env.step(2)[1:4] == (1.0, True, False)
    assert clone.get_state() == saved
    env.set_state(saved)
    for action in [0, 1]:
        env.step(action)
    assert env.step(2)[1:4] == (1.0, True, False)


def test_random_opponent_and_invalid_policy_checkpoint_atomicity():
    env = make("connect4")
    env.reset(seed=81)
    saved = env.get_state()
    expected = env.step(3)
    expected_state = env.get_state()
    env.set_state(saved)
    actual = env.step(3)
    np.testing.assert_array_equal(actual[0], expected[0])
    assert actual[1:4] == expected[1:4]
    assert env.get_state() == expected_state
    invalid = copy.deepcopy(saved)
    invalid["native"]["engine_version"] = "wrong"
    with pytest.raises(ValueError):
        env.set_state(invalid)
    assert env.get_state() == expected_state

    bad = make("tictactoe", opponent=lambda obs, info: 999)
    bad.reset(seed=3)
    before = bad.get_state()
    with pytest.raises(ValueError):
        bad.step(0)
    assert bad.get_state() == before


def test_invalid_parameters_and_reset_options_are_explicit():
    with pytest.raises(ValueError):
        make("tictactoe", seat=2)
    env = make("tictactoe")
    with pytest.raises(ResetNeeded):
        env.step(0)
    with pytest.raises(ValueError):
        env.reset(options={"unsupported": True})
    env.reset(seed=0)
    with pytest.raises(ValueError):
        env.step(True)
