# Comparing your model snapshots

```python
from gamesforai import ChatPolicy
from gamesforai.tournament import Agent, run_round_robin
from gamesforai.parquet import write_parquet

agents = [
    Agent("my-checkpoint-v1", policy_factory=lambda seed, seat:
          ChatPolicy("http://localhost:8000/v1", "my-checkpoint-v1")),
    Agent("minimax-level-3", opponent="minimax:3"),
]
report = run_round_robin("connect4", agents, games_per_pair=100, seed=42)
```

For a Python model, return a callable `(observation, info) -> action_index` from
the factory. The factory runs once per game with its seed and seat; reset recurrent
state and policy RNG there. Keep weights frozen during evaluation and give every
checkpoint a distinct stable agent id. Training may run between tournaments.

Schedules alternate seats for every pair. Built-in random/minimax/MCTS policies
are available without model dependencies. A pinned `position_set` can supply
curriculum starts, and its content hash appears in the report.

Every completed, non-truncated game contributes to one tournament-wide Glicko-2
period using pre-tournament opponent ratings. Failures, truncated episodes and
already-finished starts are counted separately and remain unrated. Reports
contain outcomes, rating intervals, win-rate Wilson intervals, seeds and trajectory
hashes. Intervals summarize this schedule; repeated/correlated positions can
make them optimistic. Engine levels remain uncalibrated without measured anchors.

Use `on_episode(summary, rows)` to write completed trajectories to JSONL or
Parquet; the runner retains summaries rather than every episode's tensors.
Sink failures abort the run. Local Python policies run in the trainer process;
they must return promptly. The HTTP adapter has its own socket timeout.

Native policy decisions and datasets repeat with fixed seeds. Full report hashes
also require the same explicit `run_id` and `created_at` values. Remote model
sampling and caller-owned model state are controlled by your model host/trainer.
