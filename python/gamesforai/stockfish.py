"""Host-owned Stockfish resources shared by local training environments."""
from __future__ import annotations

import hashlib
import os
from pathlib import Path

from ._native import NativeUciPool


class StockfishPool:
    """A reusable Linux sandbox pool with a pinned binary identity."""

    def __init__(self, engine_path=None, *, workers=1, time_ms=5000):
        engine_path = engine_path or os.environ.get("GFA_STOCKFISH_PATH")
        if not engine_path:
            raise ValueError("provide engine_path or set GFA_STOCKFISH_PATH")
        if type(workers) is not int or not 1 <= workers <= 4:
            raise ValueError("workers must be 1..4")
        if type(time_ms) is not int or not 1 <= time_ms <= 60000:
            raise ValueError("time_ms must be 1..60000")
        path = Path(engine_path)
        if not path.is_absolute():
            raise ValueError("Stockfish engine_path must be absolute")
        self._native = NativeUciPool(str(path), workers)
        digest = hashlib.sha256()
        try:
            with path.open("rb") as file:
                for chunk in iter(lambda: file.read(1024 * 1024), b""):
                    digest.update(chunk)
        except OSError as error:
            raise ValueError("Stockfish engine binary is unavailable") from error
        self._sha256 = digest.hexdigest()
        self._time_ms = time_ms

    def identity(self):
        """No machine-specific paths or credentials enter a checkpoint."""
        return {"engine_sha256": self._sha256, "preset_version": 1, "time_ms": self._time_ms}

    def choose(self, env, seat, level, seed):
        return self._native.choose(env, seat, level, seed, self._time_ms)
