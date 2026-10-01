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
