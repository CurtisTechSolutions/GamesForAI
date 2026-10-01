import json

import numpy as np
import pytest

from gamesforai import NativeEnv, games, make


@pytest.mark.parametrize("game", games())
def test_action_catalog_matches_mask_and_all_encodings(game):
    env = NativeEnv(game)
    spec = json.loads(env.spec_json())
    assert spec["id"] == game and spec["rules_markdown"] and spec["action_notation"]
    seat = env.current_players()[0]
    frame = env.frame(seat)
    catalog = frame["legal_actions"]
    assert len({index for index, _ in catalog}) == len(catalog)
    assert sorted(index for index, _ in catalog) == np.flatnonzero(frame["action_mask"]).tolist()
    assert [text for _, text in catalog] == sorted(text for _, text in catalog)
    for index, text in catalog[:16]:
        by_index, by_text = env.clone(), env.clone()
        assert by_index.step(seat, index) == by_text.step_string(seat, text)
        assert by_index.get_state() == by_text.get_state()
    if env.num_players == 2:
        assert env.frame(1 - seat)["legal_actions"] == []


def test_gym_opponent_and_learner_receive_readable_seat_scoped_moves():
    received = []
    def opponent(obs, info):
        received.append(info)
        return info["legal_actions"][0][0]

    env = make("chess", opponent=opponent)
    _, info = env.reset(seed=1)
    assert info["game_id"] == "chess"
    assert "e2e4" in [text for _, text in info["legal_actions"]]
    env.step(next(index for index, text in info["legal_actions"] if text == "e2e4"))
    assert received[0]["seat"] == 1
    assert "e7e5" in [text for _, text in received[0]["legal_actions"]]
    assert info["rules"] and info["action_notation"]
