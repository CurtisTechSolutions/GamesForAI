import sys

import pytest

from gamesforai import NativeEnv
from gamesforai._native import NativeUciPool


def test_unsupported_platform_is_explicit():
    if sys.platform == "linux":
        pytest.skip("Linux supports the sandbox")
    with pytest.raises(ValueError, match="Linux"):
        NativeUciPool("/usr/games/stockfish")


@pytest.mark.skipif(sys.platform != "linux", reason="Linux sandbox validation")
def test_invalid_pool_or_request_preserves_engine():
    for path, workers in [("relative-engine", 1), ("/missing-engine", 0), ("/missing-engine", 5)]:
        with pytest.raises(ValueError):
            NativeUciPool(path, workers)
    pool = NativeUciPool("/missing-gamesforai-test-engine")
    chess = NativeEnv("chess")
    before = chess.get_state()
    for kwargs in [{"level": 0}, {"level": 11}, {"time_ms": 0}, {"time_ms": 60001}, {"seat": 1}]:
        with pytest.raises(ValueError):
            pool.choose(chess, **{"seat": 0, **kwargs})
        assert chess.get_state() == before
    with pytest.raises(ValueError):
        pool.choose(chess, 0)
    assert chess.get_state() == before
    with pytest.raises(ValueError):
        pool.choose(NativeEnv("connect4"), 0)
