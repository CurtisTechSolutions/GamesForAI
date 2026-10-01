# Native training environments

`gfa_core::Env<G>` owns a typed engine state and runs without a server, database, or JSON serialization on each step. Use the game crate directly:

```rust
use gfa_core::{Env, Viewer};
use gfa_game_connect4::Connect4;

let mut env = Env::<Connect4>::new(42)?;
let seat = env.current_players()[0];
let mask = env.action_mask(seat)?;
let transition = env.step_index(seat, 3)?; // zero-based index: column 4
let observation = env.observe(Viewer::Player(seat))?;
# Ok::<(), gfa_core::GameError>(())
```

Actions are explicit about the acting seat. Rewards are per-seat deltas of engine returns; the current games use sparse terminal rewards. Rules endings set `terminated`; length caps set `truncated`. Reset before stepping an ended episode. An illegal action leaves state unchanged.

`clone()` makes an independent state for tree search. `get_state()` exports a versioned checkpoint and `set_state()` validates its game, engine version, configuration, and complete state before replacement. Checkpoints include private state and randomness: they are trusted training artifacts, never policy observations.

`reset(seed, Some(position))` imports a position and reseeds future randomness. Restoring a checkpoint instead preserves its exact RNG continuation. Both operations are atomic on failure.

Python Gymnasium/PettingZoo wrappers, parallel vector environments, model endpoints, and dataset exports are subsequent integration units.
