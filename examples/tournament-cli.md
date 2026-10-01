# Evaluate saved models

Install a built wheel with `pip install 'gamesforai[tournaments,datasets]'`.
Use `gfa-tournament`, `python -m gamesforai.tournament_cli`, or
`gfa tournament`. The Rust CLI forwards arguments without a shell to the SDK
in `GFA_PYTHON` (one executable path; default `python3`, or `python` on Windows).
The Python package and Rust engine should be from the same revision.

```sh
gfa tournament --game connect4 --agents examples/agents/random.yaml \
  --opponents random,minimax:1..3 --games 100 --seed 42 \
  --report report.json --trajectories trajectories --format parquet

gfa tournament --game chess --agents examples/agents/http.yaml \
  --opponents stockfish:1..8 --games 100 --stockfish /usr/games/stockfish \
  --report chess-report.json
```

`--agents a.yaml,b.yaml` accepts JSON or safe YAML. Without `--opponents`,
all agent pairs play each other. With opponents, only each supplied agent
against each selected opponent is scheduled. `--games` is an **even count per
pair**, including both seats, not a total across the run. Runs are limited to
32 agents and 10,000 games. Native ladders are provisional search presets;
no unmeasured Elo or fixed anchor rating is assigned.

Each file has a unique frozen snapshot `id` and one of:
- `type: builtin`, `opponent: random|minimax:3|mcts:5|stockfish:5`.
- `type: chat`, `base_url`, `model`, optional `api_key_env`, `timeout`,
  `max_tokens`, `temperature` and `structured`. This uses the
  [HTTP policy](../python/README.md); no request is made while loading configuration.
- `type: python`, `factory: my_package.models:load_policy`, optional
  `params: {checkpoint: weights/model.pt}`.

Python factories are explicit imports of your own executable model code.
Install the module into the selected environment, and implement:
```python
def load_policy(*, seed, seat, checkpoint):
    model = load_my_frozen_weights(checkpoint)
    # Seed your model and any framework-owned RNG here.
    def choose(observation, info):
        return int(model.choose(observation, info["action_mask"]))
    return choose
```
The runner creates a new policy per game; tensor observations and legal masks
use the same SDK contract as training. Keep weights fixed throughout evaluation.
Policy code runs in this process and must return promptly. HTTP policies use
their configured socket timeout. The CLI does not impose an arbitrary Python
inference deadline.

Optional `initial_rating: {rating: 1500, rd: 350, volatility: 0.06, games_played: 0}`
continues a game's rating pool. Only supply `fixed: true` with a measured,
explicitly calibrated anchor. Keep game configurations and assistance pools
separate. Ratings are computed locally; no server registry is updated.

`--config settings.yaml` supplies game settings; `--position-set name@version`
pins a bundled curriculum. Stockfish requires Linux, bubblewrap and working
user namespaces. All engine opponents share one pool; `--stockfish-workers`
is 1..4 and `--stockfish-time-ms` is 1..60000. Engine identity is recorded.
Stockfish's internal randomness is not controlled by the tournament seed.

The report contains match outcomes, seeds, seat counts, rating and win-rate
intervals, engine/curriculum identities and exported artifact paths. Only
completed games update ratings. Truncated and failed games have separate
counts; failures do not become losses or draws. A completed run containing
policy failures or invalid starts writes its report and exits 1. Configuration
errors exit 2; aborted runs exit 1 without replacing an existing report.

The report is replaced atomically after the run. Its parent must exist.
The optional trajectory directory must be **new**, preventing mixed runs;
each completed/truncated episode is saved atomically as JSONL or Parquet.
An aborted run can leave completed episode files, but no new complete report.
Rows include outcome and transition data, not HTTP transcripts. Configuration
files are limited to 64 KiB and 32 nesting levels; duplicate keys, aliases,
object tags, non-finite numbers and unknown agent fields are rejected.
