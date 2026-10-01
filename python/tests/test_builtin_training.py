import json

import numpy as np
import pytest

from gamesforai import NativeEnv, make


@pytest.mark.parametrize("algorithm", ["minimax", "mcts"])
def test_builtin_search_is_read_only_and_finds_immediate_win(algorithm):
    env = NativeEnv("connect4")
    for action in ["1", "2", "1", "2", "1", "3"]:
        env.step_string(env.current_players()[0], action)
    before = env.get_state()
    action = env.builtin_action(0, algorithm, 7, 42)
    assert action == 0
    assert env.builtin_action(0, algorithm, 7, 42) == action
    assert env.get_state() == before
    with pytest.raises(ValueError):
        env.builtin_action(1, algorithm, 7, 42)
    with pytest.raises(ValueError):
        env.builtin_action(0, algorithm, 11, 42)


@pytest.mark.parametrize("algorithm", ["minimax", "mcts"])
def test_capped_chess_runs_against_builtin_and_restores_reproducibly(algorithm):
    env = make("chess", config={"max_plies": 4}, opponent=f"{algorithm}:2")
    obs, info = env.reset(seed=2)
    snapshot = env.get_state()
    action = int(np.flatnonzero(info["action_mask"])[0])
    first = env.step(action)
    advanced = env.get_state()
    env.set_state(snapshot)
    second = env.step(action)
    np.testing.assert_array_equal(first[0], second[0])
    assert first[1:4] == second[1:4]
    assert env.get_state() == advanced
    _, _, terminated, truncated, _ = env.step(int(np.flatnonzero(second[4]["action_mask"])[0]))
    assert truncated and not terminated


def test_second_seat_and_invalid_opponent_configuration():
    env = make("tictactoe", seat=1, opponent="minimax:3")
    _, info = env.reset(seed=9)
    assert info["to_act"] == [1]
    assert np.count_nonzero(info["action_mask"]) == 8
    for opponent in ["minimax:0", "mcts:11", "missing:3"]:
        with pytest.raises(ValueError):
            make("connect4", opponent=opponent)
    with pytest.raises(ValueError):
        make("sudoku", opponent="minimax:3")
