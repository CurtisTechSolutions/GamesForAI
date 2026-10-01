# Training against Stockfish

```python
from gamesforai import StockfishPool, make

pool = StockfishPool("/usr/games/stockfish", workers=2, time_ms=5000)
env = make("chess", seat=0, opponent="stockfish:5", stockfish_pool=pool)
obs, info = env.reset(seed=42)
```

Without an explicit pool, set `GFA_STOCKFISH_PATH` to an absolute binary path.
Share one pool across environments to bound total engine processes. The
production sandbox requires Linux, bubblewrap, util-linux and allowed user
namespaces; the Python wheel alone does not install those host dependencies.

Stockfish levels 1–10 use the same provisional skill and node/depth presets as
the server. Time limits include startup, and engine failures roll back the
entire learner/opponent exchange or reset opening.

Checkpoints bind the binary SHA-256, preset version, level and time budget.
Restoring into different settings fails. The installed binary must remain
unchanged for the lifetime of its pool; recreate the pool after an upgrade.
Stockfish's internal random state is not exposed by UCI, so low-skill
decisions and time-limited searches are not promised to replay exactly.
Use seeded native minimax/MCTS for deterministic opponent continuation.
