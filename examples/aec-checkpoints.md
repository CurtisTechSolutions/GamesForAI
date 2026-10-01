# Resuming self-play

`aec(...).get_state()` returns a JSON-compatible checkpoint. Restore it with
`set_state(checkpoint)` in an environment with the same game and configuration.
This preserves the engine, curriculum dataset and RNG, current turn, pending
cumulative rewards, and terminal agents still waiting for their final `step(None)`.
Restoration validates the complete candidate before committing it.

```python
import json
import gamesforai

env = gamesforai.aec("connect4")
env.reset(seed=42)
checkpoint = json.loads(json.dumps(env.get_state()))
resumed = gamesforai.aec("connect4")
resumed.set_state(checkpoint)
```

Checkpoints contain private game state and are for trusted training processes.
Give each policy only `env.observe(agent)` and its own `env.infos[agent]`.
Model weights, optimizer state, policy RNGs, and Gym space sampling RNGs are
owned by the training program and must be saved alongside the environment.
Rendered information is reconstructed from the engine on restore; custom
caller-added `infos` fields are not checkpointed.
