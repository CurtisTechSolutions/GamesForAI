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

## Versioned positions

`PositionSet.load("chess-endgames-basic@1")` loads a bundled, hash-verified dataset and validates every entry using the native engine. `PositionSet.from_files(manifest_path, jsonl_path)` loads a custom set with the same checks. Entries are immutable; use `sample(numpy_rng)` to choose a deterministic starting position. Pass its `position` to a native or Gymnasium reset. Expected puzzle answers stay in trusted dataset metadata, outside policy observations. See [position data](../positions/README.md) for the format and provenance.

## Readable actions for model policies

Native frames include `legal_actions` as `(index, canonical_notation)` pairs in stable notation order. Gymnasium info includes the same catalog plus the game ID, rules, and action notation for learner and opponent policies. The catalog contains only actions available to that seat and matches its numeric mask. `NativeEnv.spec_json()` returns the static game contract. Vector batches keep their numeric-only output.

## Sampling a curriculum on every reset

Use `env.reset(seed=42, options={"position_set": "chess-endgames-basic@1"})` with either Gymnasium or AEC. Later `reset()` calls keep sampling that pinned set; repeating an explicit seed repeats the selection. Supply a custom `PositionSet` instead of a bundled name, or use `{"position_set": None}` to return to initial positions. An explicit `position` also clears the active curriculum. A set must match the environment game and normalized configuration.

`info["curriculum"]` records only the set ID, content hash, and selected position ID. Expected puzzle answers are excluded. Gymnasium checkpoints include the pinned dataset and RNG continuation; AEC clones preserve both. Invalid selections, imported positions, and failed opponent openings preserve environment and RNG state. A caller-owned opponent model must manage its own mutable state.

## Built-in training opponents

Use `make("connect4", opponent="minimax:3")` or `opponent="mcts:3"` for the native built-in search algorithms at levels 1–10. Training uses fixed node/depth budgets and RNG seeds, so choices do not depend on machine speed. Search reconstructs a planning state from the opponent seat’s observation and never mutates the live environment. Gymnasium checkpoints preserve the opponent seed stream. These are the existing provisional resource levels; their playing strength has not yet been calibrated.

For an independent native environment, `builtin_action(seat, algorithm, level, seed)` returns a legal index without stepping it. Search supports the same perfect-information sequential games as the shared opponent implementation. External Stockfish training integration is a separate adapter.

## Your own HTTP model

```python
from gamesforai import ChatPolicy, make

policy = ChatPolicy("http://localhost:8000/v1", "your-model", api_key_env="MY_MODEL_KEY")
env = make("connect4", opponent=policy)
obs, info = env.reset(seed=42)
# Or choose learner moves directly with policy(obs, info).
```

ChatPolicy uses the chat-completions protocol implemented by servers such as [vLLM](https://docs.vllm.ai/en/latest/serving/online_serving/). It sends only seat-visible text, rules, and canonical legal moves. The model returns `{"action":"4"}` for Connect Four column 4, or `{"action":"e2e4"}` for a chess move. The adapter validates the move and converts it to the native index.

Credentials are read from the named environment variable on each call; omit `api_key_env` for a server without authentication. No model or provider account is required by the engine. Structured output uses a JSON Schema with the legal moves as an enum, as supported by [vLLM structured outputs](https://docs.vllm.ai/en/latest/features/structured_outputs/). Set `structured=False` for a server that accepts the chat protocol but not that parameter; the reply must still be the exact JSON object.

Requests and replies are capped at 64 KiB; output tokens and the socket timeout are configurable. Calls do not retry or follow redirects. `PolicyError.code` distinguishes malformed/illegal replies, authentication, rate limits, timeouts, and connection failures without exposing response bodies or credentials. A Gymnasium opponent failure rolls back the complete learner/opponent exchange. This synchronous adapter is for local training/evaluation code; durable server-side LLM transcripts and budgets are a separate integration.
