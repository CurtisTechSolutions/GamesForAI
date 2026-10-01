import json

import numpy as np
import pytest

from gamesforai import VectorEnv, connect
from gamesforai.remote import RemoteError
from test_remote import server  # Shared loopback protocol fixture.


def equal(left, right):
    assert left.keys() == right.keys()
    for key in left:
        assert left[key].dtype == right[key].dtype
        np.testing.assert_array_equal(left[key], right[key])


def test_remote_vector_matches_native_complete_games_and_explicit_views(server):
    url, _ = server
    remote = connect(url).vector("tictactoe", 4, seed=42, batch_size=2)
    local = VectorEnv("tictactoe", 4, seed=42)
    equal(remote.frame(), local.frame())
    equal(remote.frame([1, 0, 1, 0]), local.frame([1, 0, 1, 0]))
    for _ in range(10):
        frame = local.frame()
        actions = [None if ended else int(np.flatnonzero(mask)[0])
                   for ended, mask in zip(frame["terminated"] | frame["truncated"], frame["action_mask"])]
        equal(remote.step(actions), local.step(actions))
        assert [json.loads(value) for value in remote.get_state()] == [
            json.loads(value) for value in local.get_state()]
        if np.all(local.frame()["terminated"]):
            break
    assert np.all(remote.frame()["terminated"])
    equal(remote.step([None] * 4), local.step([None] * 4))


def test_late_chunk_failure_preserves_every_checkpoint_and_can_retry(server):
    url, state = server
    env = connect(url).vector("tictactoe", 65, batch_size=32)
    env.step([0] * 65)
    before = env.get_state()
    calls = len(state.requests)
    # The occupied square fails in the third chunk, after earlier chunks succeeded.
    with pytest.raises(RemoteError, match="ILLEGAL_ACTION"):
        env.step([1] * 64 + [0])
    assert env.get_state() == before
    requests = [body for _, _, body in state.requests[calls:] if body is not None]
    assert [len(body["operations"]) for body in requests] == [32, 32, 1]
    env.step([1] * 65)
    assert env.get_state() != before


def test_partial_reset_curriculum_and_checkpoint_clone_parity(server):
    url, _ = server
    remote = connect(url).vector("connect4", 3, seed=8)
    local = VectorEnv("connect4", 3, seed=8)
    options = {"position_set": "connect4-solved-positions@1"}
    equal(remote.reset(seed=42, options=options), local.reset(seed=42, options=options))
    assert remote.origins == local.origins
    checkpoint = remote.get_state()
    clone = remote.clone()
    equal(remote.reset_at([2, 0], [8, 7]), local.reset_at([2, 0], [8, 7]))
    assert clone.get_state() == checkpoint
    assert remote.get_state() != checkpoint
    remote.set_state(checkpoint)
    equal(remote.frame(), clone.frame())
    bad = remote.get_state()
    bad["native"][2] = "{}"
    with pytest.raises(ValueError):
        remote.set_state(bad)
    assert remote.get_state() == checkpoint


def test_projection_failure_does_not_commit_and_arrays_are_owned(server):
    url, state = server
    env = connect(url).vector("tictactoe", 3)
    before = env.get_state()
    state.fail_observe = True
    with pytest.raises(RemoteError):
        env.step([0, 1, 2])
    assert env.get_state() == before
    state.fail_observe = False
    frame = env.frame()
    frame["action_mask"][:] = 0
    frame["observation"][:] = 99
    assert env.frame()["action_mask"].all()
    assert not (env.frame()["observation"] == 99).any()


def test_vector_rejects_invalid_batches_before_committing(server):
    url, _ = server
    client = connect(url)
    for kwargs in [{"n": 0}, {"n": 4097}, {"n": 1, "batch_size": 0},
                   {"n": 1, "threads": 2}, {"n": 2, "seed": 2**64 - 1}]:
        with pytest.raises(ValueError):
            client.vector("tictactoe", **kwargs)
    env = client.vector("tictactoe", 2)
    before = env.get_state()
    for actions in [[0], [True, 1], [None, None], [0, 10]]:
        with pytest.raises(ValueError):
            env.step(actions)
    with pytest.raises(ValueError):
        env.reset_at([0, 0], [1, 2])
    with pytest.raises(ValueError):
        env.frame([2, 0])
    assert env.get_state() == before
