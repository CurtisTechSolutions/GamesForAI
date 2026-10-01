import json

import pytest

from gamesforai.ratings import Rating, rate_pair


def test_native_rating_reference_and_json_checkpoint():
    current = Rating(rd=200)
    updated = current.update([(Rating(rating=1400, rd=30), 1),
                              (Rating(rating=1550, rd=100), 0),
                              (Rating(rating=1700, rd=300), 0)])
    assert updated.rating == pytest.approx(1464.06, abs=0.01)
    assert updated.rd == pytest.approx(151.52, abs=0.01)
    assert updated.games_played == 3
    assert Rating(**json.loads(json.dumps(updated.to_dict()))) == updated
    assert current.rd == 200


def test_pair_symmetry_anchor_and_inactive_period():
    original = Rating()
    winner, loser = rate_pair(original, original, 1)
    assert winner.rating + loser.rating == pytest.approx(3000)
    assert winner.games_played == loser.games_played == 1
    idle = winner.update()
    assert idle.rating == winner.rating and idle.rd > winner.rd
    anchor = Rating(rating=1800, rd=0)
    learner, fixed = rate_pair(original, anchor, 1, fixed_right=True)
    assert learner.rating > winner.rating
    assert fixed.rating == anchor.rating and fixed.rd == 0
    assert original.interval95 == (800, 2200)


@pytest.mark.parametrize("kwargs", [
    {"rd": -1}, {"volatility": 0}, {"rating": float("nan")},
    {"games_played": -1}, {"games_played": True},
])
def test_invalid_rating_inputs(kwargs):
    with pytest.raises(ValueError):
        Rating(**kwargs)


def test_invalid_results_and_anchor_flags():
    original = Rating()
    for score in (-1, 0.25, 2, float("inf")):
        with pytest.raises(ValueError):
            rate_pair(original, original, score)
    with pytest.raises(ValueError):
        rate_pair(original, original, 1, fixed_left=1)
    with pytest.raises(ValueError):
        original.update([(original, 1)], tau=0)
