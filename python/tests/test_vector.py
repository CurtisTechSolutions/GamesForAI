import json

import numpy as np
import pytest

from gamesforai import NativeEnv, VectorEnv


@pytest.mark.parametrize("game,config", [
    ("tictactoe", {}),
    ("connect4", {}),
    ("chess", {"max_plies": 8}),
    ("sudoku", {"size": 4, "max_moves": 8, "allow_notes": False}),
])
def test_parallel_batch_matches_individual_engines(game, config):
    batch = VectorEnv(game, 8, config=config, seed=40, threads=2)
    singles = [NativeEnv(game, json.dumps(config), 40 + i) for i in range(8)]
    rng = np.random.default_rng(9)
    for _ in range(100):
        frame = batch.frame()
        assert frame["observation"].dtype == np.float32
        assert frame["action_mask"].dtype == np.int8
        assert frame["terminated"].dtype == np.bool_
        actions, expected_rewards = [], []
        for i, env in enumerate(singles):
            actors = env.current_players()
            seat = actors[0] if actors else 0
            np.testing.assert_array_equal(frame["observation"][i], env.frame(seat)["observation"])
            np.testing.assert_array_equal(frame["action_mask"][i], env.frame(seat)["action_mask"])
            assert frame["to_act"][i] == (seat if actors else -1)
            if actors:
                action = int(rng.choice(np.flatnonzero(frame["action_mask"][i])))
                actions.append(action)
                rewards, _, _ = env.step(seat, action)
                expected_rewards.append(rewards)
            else:
                actions.append(None)
                expected_rewards.append([0.0] * env.num_players)
        result = batch.step(actions)
        np.testing.assert_array_equal(result["rewards"], expected_rewards)
        assert batch.get_state() == [env.get_state() for env in singles]
        if np.all(result["terminated"] | result["truncated"]):
            break
    else:
        pytest.fail("batch did not finish within game bounds")


def test_batch_errors_reset_and_checkpoint_are_atomic():
    batch = VectorEnv("connect4", 4, seed=12, threads=2)
    before = batch.get_state()
    with pytest.raises(ValueError):
        batch.step([0, 1, 99, 3])
    assert batch.get_state() == before
    with pytest.raises(ValueError):
        batch.reset_at([0, 1], [30, 31], positions=[None, "invalid"])
    assert batch.get_state() == before
    batch.step([0, 1, 2, 3])
    played = batch.get_state()
    clone = batch.clone()
    clone.reset_at([1, 3], [51, 53])
    assert clone.get_state()[0] == played[0]
    assert clone.get_state()[2] == played[2]
    assert clone.get_state()[1] != played[1]
    assert batch.get_state() == played
    broken = list(before)
    broken[2] = "{}"
    with pytest.raises(ValueError):
        batch.set_state(broken)
    assert batch.get_state() == played
    batch.set_state(before)
    assert batch.get_state() == before
    np.testing.assert_array_equal(batch.reset(12)["observation"], VectorEnv("connect4", 4, seed=12).frame()["observation"])
    with pytest.raises(ValueError):
        batch.reset_at([0, 0], [1, 2])


def test_terminal_rows_are_not_automatically_reset():
    batch = VectorEnv("tictactoe", 2)
    for action in [0, 3, 1, 4, 2]:
        result = batch.step([action, action])
    np.testing.assert_array_equal(result["rewards"], [[1, -1], [1, -1]])
    assert result["terminated"].all()
    assert not result["action_mask"].any()
    before = batch.get_state()
    with pytest.raises(ValueError):
        batch.step([0, None])
    assert batch.get_state() == before
    assert not batch.step([None, None])["rewards"].any()
    reset = batch.reset_at([0], [99])
    np.testing.assert_array_equal(reset["terminated"], [False, True])
    batch.step([0, None])


def test_thread_count_does_not_change_results_and_arrays_own_storage():
    serial = VectorEnv("connect4", 32, seed=91, threads=1)
    parallel = VectorEnv("connect4", 32, seed=91, threads=4)
    for column in [0, 1, 2, 3]:
        left, right = serial.step([column] * 32), parallel.step([column] * 32)
        for key in left:
            np.testing.assert_array_equal(left[key], right[key])
    before = parallel.get_state()
    frame = parallel.frame()
    frame["observation"][:] = 123
    frame["action_mask"][:] = 0
    assert parallel.get_state() == before
    assert not parallel.frame([1] * 32)["action_mask"].any()
    with pytest.raises(ValueError):
        parallel.frame([2] * 32)
    with pytest.raises(ValueError):
        VectorEnv("connect4", 0)
    with pytest.raises(ValueError):
        VectorEnv("connect4", 2, seed=2**64 - 1)
