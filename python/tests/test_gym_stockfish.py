import hashlib

import numpy as np
import pytest

from gamesforai import StockfishPool, make
import gamesforai.stockfish as stockfish_module


@pytest.fixture
def pool(tmp_path, monkeypatch):
    class FixturePool:
        def __init__(self, path, workers):
            self.fail = False
            self.calls = []

        def choose(self, native, seat, level, seed, time_ms):
            self.calls.append((seat, level, seed, time_ms))
            if self.fail:
                raise ValueError("fixture engine timeout")
            return int(np.flatnonzero(native.frame(seat)["action_mask"])[0])

    monkeypatch.setattr(stockfish_module, "NativeUciPool", FixturePool)
    path = tmp_path / "fixture-engine"
    path.write_bytes(b"training-engine-identity")
    return StockfishPool(path, time_ms=1000)


def test_stockfish_openings_shared_pool_and_checkpoint_identity(pool):
    env = make("chess", seat=1, opponent="stockfish:5", stockfish_pool=pool)
    _, info = env.reset(seed=92)
    assert info["to_act"] == [1]
    assert pool._native.calls[-1][0:2] == (0, 5)
    state = env.get_state()
    identity = state["opponent"]
    assert identity == {
        "kind": "stockfish", "level": 5, "preset_version": 1, "time_ms": 1000,
        "engine_sha256": hashlib.sha256(b"training-engine-identity").hexdigest(),
    }
    clone = env.clone()
    assert clone._stockfish is pool
    action = int(np.flatnonzero(info["action_mask"])[0])
    env.step(action)
    clone.step(action)
    assert env.get_state() == clone.get_state()
    restored = make("chess", seat=1, opponent="stockfish:5", stockfish_pool=pool)
    restored.set_state(state)
    assert restored.get_state() == state
    with pytest.raises(ValueError, match="opponent"):
        make("chess", seat=1, opponent="stockfish:4", stockfish_pool=pool).set_state(state)


def test_failed_opponent_exchange_and_opening_are_atomic(pool):
    env = make("chess", opponent="stockfish:2", stockfish_pool=pool)
    _, info = env.reset(seed=3)
    before = env.get_state()
    pool._native.fail = True
    with pytest.raises(ValueError, match="timeout"):
        env.step(int(np.flatnonzero(info["action_mask"])[0]))
    assert env.get_state() == before
    second = make("chess", seat=1, opponent="stockfish:2", stockfish_pool=pool)
    before = second.get_state()
    with pytest.raises(ValueError, match="timeout"):
        second.reset(seed=3)
    assert second.get_state() == before


def test_bad_game_settings_and_missing_binary_are_explicit(pool, monkeypatch):
    with pytest.raises(ValueError, match="chess"):
        make("connect4", opponent="stockfish:5", stockfish_pool=pool)
    with pytest.raises(ValueError, match="requires"):
        make("chess", stockfish_pool=pool)
    monkeypatch.delenv("GFA_STOCKFISH_PATH", raising=False)
    with pytest.raises(ValueError, match="GFA_STOCKFISH_PATH"):
        make("chess", opponent="stockfish:1")
    with pytest.raises(ValueError, match="absolute"):
        StockfishPool("relative")
    with pytest.raises(ValueError, match="workers"):
        StockfishPool("/missing", workers=0)
    with pytest.raises(ValueError, match="time_ms"):
        StockfishPool("/missing", time_ms=60001)


def test_changed_engine_identity_rejects_restore(pool, tmp_path):
    env = make("chess", opponent="stockfish:5", stockfish_pool=pool)
    env.reset(seed=2)
    path = tmp_path / "different-engine"
    path.write_bytes(b"different model")
    other_pool = StockfishPool(path, time_ms=1000)
    other = make("chess", opponent="stockfish:5", stockfish_pool=other_pool)
    before = other.get_state()
    with pytest.raises(ValueError, match="opponent"):
        other.set_state(env.get_state())
    assert other.get_state() == before
