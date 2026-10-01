# Loading trajectories into your trainer

Install the optional dependency with `python -m pip install '.[datasets]'`
when building from this repository, or install the wheel with `[datasets]`.

```python
import numpy as np
import pyarrow.parquet as pq
from gamesforai.parquet import write_parquet
from gamesforai.trajectories import select_records

# episode is an EpisodeRecorder that has finished.
write_parquet("train.parquet", select_records(episode.records(), agent="your-model-v1"))
for batch in pq.ParquetFile("train.parquet").iter_batches(batch_size=256):
    for row in batch.to_pylist():
        obs = np.asarray(row["observation"], dtype=np.float32).reshape(row["observation_shape"])
        mask = np.asarray(row["action_mask"], dtype=bool)
        # Feed obs, mask, action_index, rewards and episode_returns to your trainer.
```

The versioned schema stores observations as float32 lists, masks as boolean
lists, rewards/returns as float64 lists, and shape/action/seat metadata as
integers. Seed strings preserve all 64 bits across JSON consumers. The schema
also retains text, reasoning and optional transcript JSON from local recording.

Export validates shapes, masks, actions, rewards and flags before writing.
Zstandard-compressed row groups stream from the supplied iterable; batch size
bounds row count, not total byte size. Failures leave an existing output file
intact. Empty datasets still carry the complete schema.

Every row remains one game turn. The next observation belongs to the same
acting seat, whose next mask may be empty while another seat acts. Construct
multi-agent decision transitions before applying a single-agent RL objective.

The implementation uses the documented [PyArrow ParquetWriter](https://arrow.apache.org/docs/python/generated/pyarrow.parquet.ParquetWriter.html)
and explicit Arrow schemas so nullable first rows do not change column types.
