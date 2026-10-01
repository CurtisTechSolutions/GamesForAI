# Rating model snapshots

```python
from gamesforai.ratings import Rating, rate_pair

model_v1, model_v2 = Rating(), Rating()
model_v1, model_v2 = rate_pair(model_v1, model_v2, 1.0)  # v1 won
print(model_v1.rating, model_v1.interval95)
checkpoint = model_v1.to_dict()
restored = Rating(**checkpoint)
```

These Python functions call the same Rust Glicko-2 implementation as the service
domain. Use stable model snapshot identifiers and separate rating pools for
each game, rules configuration and assistance policy. Start uncalibrated
players with the default prior. Fixed anchors require measured calibration.

Use score 1/0.5/0 for win/draw/loss. Exclude truncated and failed games instead
of treating them as draws. For a whole rating period, pass each player's
results with opponents' pre-period ratings to `Rating.update`; do not substitute
ratings that were already updated during that period. Calling `update()` with
no games represents one explicit inactive period and increases uncertainty.

Rating dictionaries are portable checkpoint data. This local API does not
write a server leaderboard or manufacture calibrated engine ratings.
