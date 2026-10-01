# Native Python Stockfish pool

The Python bridge reuses the same bounded Linux engine pool as the server.
Install Stockfish, bubblewrap and util-linux, and permit the required user
namespaces for bubblewrap in your host policy.

```python
from gamesforai import NativeEnv
from gamesforai._native import NativeUciPool

pool = NativeUciPool("/usr/games/stockfish", workers=1)
env = NativeEnv("chess", seed=42)
action = pool.choose(env, 0, level=5, seed=42, time_ms=5000)
env.step(0, action)
```

Only the acting seat's observation and legal actions enter planning. Engine
moves and principal variations are checked against the rules before an action
is returned. A failed search leaves the environment unchanged. The interpreter
is released during the search; a shared pool has 1–4 workers and rejects work
when all are busy. Deadlines include startup, synchronization and search.

Levels use the server's provisional skill and node/depth presets. The seed
belongs to GamesForAI planning; Stockfish does not expose its internal random
state through UCI. Low-skill play and time-limited searches are not guaranteed
to replay exactly. Windows and macOS report unsupported sandbox availability.
Use the native Python policies or HTTP model adapter on those platforms.
