# Training examples

Install a wheel with optional training dependencies, or run `python -m pip install '.[training]'` from the repository root. The environment itself does not require a training framework or host model weights.

```sh
python examples/train_ppo.py --game connect4 --steps 250000 --envs 8 --output training-output
```

The example uses [SB3-Contrib MaskablePPO](https://sb3-contrib.readthedocs.io/en/master/modules/ppo_mask.html) with a small CPU policy and GamesForAI's Gymnasium action masks. It trains both seats against the configured opponent. Replace the policy architecture or use the same environment in your own trainer; model training stays in your process.

`--opponent minimax:3` trains against that native search baseline. `--eval-opponent minimax:3` selects the comparison baseline; evaluation also runs against random play. Fixed held-out seeds, balanced seats, and two random opening plies by default create repeatable evaluation positions. `--opening-plies 0` evaluates only standard starts. Reports include sample size, outcomes, win-rate intervals, raw mean returns, package versions, seeds, and training configuration. Small smoke-run estimates do not establish model strength.

`policy.zip` is saved and loaded before evaluation. `report.json` records changed parameters, exact checkpoint verification, and a repeated evaluation check. CI runs actual training with a small budget and uploads those artifacts; the larger PRD acceptance run against level 3 is separate from this integration test. The example uses SB3's environment batching; native VectorEnv is available for custom batched self-play trainers.

```python
from sb3_contrib import MaskablePPO
from gamesforai import make

model = MaskablePPO.load("training-output/policy.zip", device="cpu")
env = make("connect4", opponent="minimax:3")
obs, info = env.reset(seed=123)
action, _ = model.predict(obs, action_masks=env.action_masks(), deterministic=True)
obs, reward, terminated, truncated, info = env.step(int(action))
```
