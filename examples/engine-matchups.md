# Selected model matchups

```python
from gamesforai import StockfishPool
from gamesforai.tournament import Agent, run_round_robin

pool = StockfishPool("/usr/games/stockfish", workers=2)
agents = [Agent("my-model-v1", policy_factory=my_model_factory)] + [
    Agent(f"stockfish-{level}", opponent=f"stockfish:{level}", stockfish_pool=pool)
    for level in range(1, 9)
]
report = run_round_robin("chess", agents, games_per_pair=100,
                        matchups=[("my-model-v1", f"stockfish-{level}") for level in range(1, 9)])
```

The explicit schedule avoids engine-versus-engine games when evaluating your
model against a ladder. Each requested pair still alternates seats. Reports pin
the engine binary hash and resource-preset identity alongside the matchups.
Duplicate, self, and unknown-agent pairs are rejected before play.

Stockfish retains the same Linux sandbox requirements and UCI randomness
limitations as the Gym adapter. Its level labels are resource presets until
calibration assigns measured anchor ratings.
