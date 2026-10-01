# Batched curriculum training

```python
from gamesforai import VectorEnv

envs = VectorEnv("connect4", 256, threads=4)
batch = envs.reset(42, options={"position_set": "connect4-solved-positions@1"})
origins = envs.origins  # per-row dataset id, content hash and position id
checkpoint = envs.get_state()
envs.reset_at([0, 12], [100, 101])  # retain each selected row's curriculum
envs.set_state(checkpoint)
```

Every row samples with its explicit reset seed, so results are independent of
thread scheduling. Full resets use `seed + row`, or accept a seed list.
`options={"position_set": None}` clears curricula for selected rows.
Explicit `positions=[...]` also clears curricula for those rows.
Failed resets preserve both engine state and dataset identities.

Numeric frames keep their existing array-only schema. Read public origin
metadata separately through `origins`; expected answers stay in trusted datasets.
Curriculum checkpoints use a version 2 dictionary with each dataset stored once
and per-row references. Batches without curricula retain the original list of
native snapshots. Both forms are accepted by `set_state`.
