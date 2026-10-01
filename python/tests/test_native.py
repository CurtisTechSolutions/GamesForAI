import json
from concurrent.futures import ThreadPoolExecutor

import numpy as np
import pytest

from gamesforai import NativeEnv, games


@pytest.mark.parametrize("game", ["tictactoe", "connect4", "sudoku", "chess"])
def test_native_frames_and_illegal_action_are_atomic(game):
    config = '{"size":4}' if game == "sudoku" else "{}"
    env = NativeEnv(game, config, 17)
    assert game in games()
    seat = env.current_players()[0]
    frame = env.frame(seat)
    assert frame["observation"].dtype == np.float32
    assert frame["action_mask"].dtype == np.int8
    assert frame["action_mask"].shape == (env.action_space_size,)
    assert np.isfinite(frame["observation"]).all()
    assert frame["text"]
    saved = env.get_state()
    with pytest.raises(ValueError):
        env.step(seat, env.action_space_size)
    assert env.get_state() == saved
    # NumPy owns its returned buffers; changing them must not alter engine state.
    frame["observation"].fill(999)
    frame["action_mask"].fill(0)
    assert env.get_state() == saved
    frame = env.frame(seat)
    action = int(np.flatnonzero(frame["action_mask"])[0])
    env.step(seat, action)
    assert env.get_state() != saved


def test_clone_checkpoint_rewards_and_finished_episode():
    env = NativeEnv("tictactoe")
    env.step_string(0, "r1c1")
    saved = env.get_state()
    clone = env.clone()
    clone.step(1, 8)
    assert env.get_state() == saved
    for _ in range(2):
        env.set_state(saved)
        for seat, action in [(1, 3), (0, 1), (1, 4)]:
            assert env.step(seat, action) == ([0.0, 0.0], False, False)
        assert env.step(0, 2) == ([1.0, -1.0], True, False)
        assert env.returns() == [1.0, -1.0]
        assert env.current_players() == []
        with pytest.raises(ValueError):
            env.step(1, 8)
    env.reset(0)
    assert env.flags() == (False, False)


def test_checkpoint_validation_and_truncation():
    env = NativeEnv("chess", '{"max_plies":1}')
    assert env.step_string(0, "e2e4") == ([0.0, 0.0], False, True)
    saved = env.get_state()
    env.reset()
    env.set_state(saved)
    assert env.flags() == (False, True)
    before = env.get_state()
    invalid = json.loads(saved)
    invalid["engine_version"] = "incompatible"
    with pytest.raises(ValueError):
        env.set_state(json.dumps(invalid))
    assert env.get_state() == before


def test_threads_can_drive_independent_native_states():
    def episode(seed):
        env = NativeEnv("tictactoe", seed=seed)
        while env.current_players():
            seat = env.current_players()[0]
            action = int(np.flatnonzero(env.frame(seat)["action_mask"])[0])
            env.step(seat, action)
        return env.returns()

    with ThreadPoolExecutor(max_workers=4) as pool:
        results = list(pool.map(episode, range(32)))
    assert results == [[1.0, -1.0]] * 32
