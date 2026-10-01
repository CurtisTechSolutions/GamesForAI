# Recording your models' games

```python
import numpy as np
from gamesforai import ChatPolicy
from gamesforai.trajectories import EpisodeRecorder, select_records, write_jsonl

policy = ChatPolicy("http://localhost:8000/v1", "your-model")
episode = EpisodeRecorder("connect4", ["your-model-v1", "random-v1"], seed=42)
rng = np.random.default_rng(42)
while episode.current_players:
    seat = episode.current_players[0]
    obs, info = episode.observation(seat)
    action = policy(obs, info) if seat == 0 else int(rng.choice(np.flatnonzero(info["action_mask"])))
    episode.step(seat, action)
write_jsonl("training.jsonl", select_records(episode.records(), agent="your-model-v1"))
```

Python callbacks can replace the HTTP policy directly. Each row records one
game turn: the acting seat's observation, legal mask, canonical action and index,
per-seat immediate reward vector, same-seat next observation, and next actors.
Final episode returns and each acting seat's eventual outcome are added only
after the game ends. Truncation remains distinct from a draw. Multi-agent
trainers must account for intervening opponent turns when forming decision
transitions; this format does not collapse them into a single Gym step.

The schema pins the engine version, normalized configuration, seed, episode id,
timestamp, agent identity and optional supplied rating. Ratings are metadata,
not an estimate produced by the recorder. Optional reasoning and caller-supplied
full transcripts are accepted by `step`; HTTP policy transcripts are not
automatically captured. Only provide transcript content you intend to export.

Records contain seat-visible observations rather than private engine snapshots
or position-set answers. A recorder buffers one episode with configurable
step/byte limits; a rejected action or limit leaves the episode unchanged.
Exports support game, agent, rating, date and outcome filters, and replace files
atomically. Server-owned match export and hidden-benchmark access controls are
separate service features; this API records local training games.
