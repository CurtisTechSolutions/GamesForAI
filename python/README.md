# GamesForAI Python

The native bridge keeps engine state in Rust and exposes NumPy observations and discrete action masks. Build from the repository root with `python -m pip install .` (a Rust compiler is needed for a source build). CI builds and exercises wheels on Linux, Windows, and macOS.

```python
import numpy as np
from gamesforai import NativeEnv

env = NativeEnv("connect4", seed=42)
while env.current_players():
    seat = env.current_players()[0]
    frame = env.frame(seat)
    action = int(np.flatnonzero(frame["action_mask"])[0])
    rewards, terminated, truncated = env.step(seat, action)
```

Replace the action selection with your own policy. `frame(seat)` contains a float32 tensor, an int8 action mask, text, and JSON for that seat's visible board. Invalid actions raise `ValueError` and preserve state. Native compute releases the Python interpreter while it runs. Returned arrays own their storage and cannot mutate an environment.

`clone()`, `get_state()`, and `set_state()` support local search and exact continuation. A checkpoint contains full private engine state and is for trusted training code, not a policy input. `reset(seed, position)` imports a curriculum position while reseeding future randomness.

This unit exposes the native bridge. Gymnasium/AEC convenience wrappers and parallel batch stepping follow separately.

The new PyO3 and rust-numpy dependencies provide the CPython ABI boundary and owned NumPy buffers; no hand-written unsafe code is needed. Packaging follows [Maturin's mixed project layout](https://www.maturin.rs/project_layout.html). Native calls use [PyO3 interpreter detachment](https://pyo3.rs/v0.29.2/parallelism), and arrays use [rust-numpy ownership transfer](https://docs.rs/numpy/0.29.0/numpy/array/struct.PyArray.html).

## Gymnasium and your policy

```python
import numpy as np
import gamesforai

env = gamesforai.make("connect4")
obs, info = env.reset(seed=42)
while True:
    action = your_policy(obs, info["action_mask"])
    obs, reward, terminated, truncated, info = env.step(action)
    if terminated or truncated:
        break
```

The default opponent samples legal actions from the environment's seeded random generator. Select `seat=1` to train from the second seat; the opponent plays its opening during reset. Supply `opponent=my_policy` to play against a Python callable accepting `(observation, info)` and returning a discrete index. Each opponent receives its own seat's view.

Masks are required: a fixed action space includes actions that are illegal in a particular position. Invalid actions raise `ValueError` without committing a learner/opponent exchange. Call `reset` after termination or truncation. Position curricula can use `reset(options={"position": notation})`.

Wrapper checkpoints include native state and the environment RNG, so random-opponent continuations repeat exactly. A cloned wrapper owns independent game, RNG, and action-space state. A custom policy callable and its model state remain caller-owned; checkpoint those separately for reproducible self-play.

## PettingZoo self-play

```python
import gamesforai

env = gamesforai.aec("connect4")
env.reset(seed=42)
for agent in env.agent_iter():
    obs, reward, terminated, truncated, info = env.last()
    action = None if terminated or truncated else policies[agent](
        obs["observation"], obs["action_mask"]
    )
    env.step(action)
```

Agents are named `player_0`, `player_1`, and so on. Each policy sees only its seat's observation. The AEC interface preserves accumulated rewards and gives every agent its final transition before `step(None)` removes it. Numeric spaces and masks stay fixed through an episode. `clone()` copies native and AEC bookkeeping for independent search; rendering uses the public spectator view.

CI runs [PettingZoo's API test](https://pettingzoo.farama.org/content/environment_tests/) against every installed game in addition to full-game reward and truncation tests.

## Versioned positions

`PositionSet.load("chess-endgames-basic@1")` loads a bundled, hash-verified dataset and validates every entry using the native engine. `PositionSet.from_files(manifest_path, jsonl_path)` loads a custom set with the same checks. Entries are immutable; use `sample(numpy_rng)` to choose a deterministic starting position. Pass its `position` to a native or Gymnasium reset. Expected puzzle answers stay in trusted dataset metadata, outside policy observations. See [position data](../positions/README.md) for the format and provenance.
