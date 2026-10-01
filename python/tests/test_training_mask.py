import numpy as np
import pytest
from gymnasium.error import ResetNeeded

from gamesforai import make


def test_maskable_training_api_is_boolean_owned_and_requires_active_episode():
    env = make("connect4")
    with pytest.raises(ResetNeeded):
        env.action_masks()
    _, info = env.reset(seed=1)
    mask = env.action_masks()
    assert mask.dtype == np.bool_
    np.testing.assert_array_equal(mask, info["action_mask"])
    mask[:] = False
    assert env.action_masks().all()
    _, _, _, _, info = env.step(3)
    np.testing.assert_array_equal(env.action_masks(), info["action_mask"])
