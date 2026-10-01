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

PyO3's target detection uses target-lexicon, licensed under Apache-2.0 with the LLVM exception; the dependency allowlist records that exact expression.

## Parallel batches

```python
from gamesforai import VectorEnv

batch = VectorEnv("connect4", 256, seed=42, threads=8)
frame = batch.frame()
actions = your_batched_policy(frame["observation"], frame["action_mask"], frame["seat"])
frame = batch.step(actions)
finished = frame["terminated"] | frame["truncated"]
```

Each row advances one game turn; the caller chooses policies for all acting seats.
Observations have shape `(n, *game_shape)`, masks `(n, action_space_size)`,
and rewards `(n, num_players)`. Other arrays have shape `(n,)`.
`seat` identifies the projected view; `to_act` is -1 for finished rows.
By default a live row projects its next actor and a finished row projects seat 0.
Use `frame(seats=[...])` for explicit views; the engine enforces each seat's information set.

Finished rows require `None` actions and produce zero subsequent rewards.
Record their final observations/rewards before resetting with
`reset_at(indices, seeds, positions=...)`. No implicit reset overwrites a final transition.
Batch steps, resets, and checkpoint restores commit only when every row succeeds.
Clones share a worker pool but own independent states. Seed + row initialization
and ordered outputs are independent of worker scheduling.

Native work releases the interpreter and uses a bounded [Rayon thread pool](https://docs.rs/rayon/1.12.0/rayon/struct.ThreadPool.html).
This API batches game turns, not learner/opponent exchanges from the single-agent
Gymnasium wrapper. It supports 1–4096 rows, at most 64 workers, and at most 128 MiB
of numeric output per batch. Throughput still needs measurement against the PRD target.
