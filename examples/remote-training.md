# Use the SDK against a server

Start `gfa serve --sqlite training.sqlite` and connect to its printed address:

```python
import os
import gamesforai as gfa

client = gfa.connect("http://127.0.0.1:8080")
# For an authenticated deployment: gfa.connect(url, os.environ["GFA_API_KEY"])
env = client.make("connect4", seat=0, opponent="random")
obs, info = env.reset(seed=42)
obs, reward, terminated, truncated, info = env.step(3)

checkpoint = env.get_state()
branch = env.clone()
branch.set_state(checkpoint)

multi = client.aec("tictactoe")
multi.reset(seed=42)
native = client.native("chess")
```

Gymnasium and PettingZoo use the same wrappers as local training, including
curriculum resets, reward semantics and wrapper checkpoints. Python callbacks
and `ChatPolicy` run in your training process using only the requested seat's
observation and legal moves. Random opponents use the wrapper's checkpointed
RNG. Native minimax/MCTS recommendations use the server's analysis endpoint
and its configured budgets; the same local assistance guards apply. Remote
Stockfish Gym configuration and vector wrappers are not yet available; native
Stockfish/VectorEnv remain available for in-process training.

`client.batch(operations)` exposes the [batch REST contract](../docs/training-batches.md)
directly, with up to 64 independent create/reset/step/observe operations. It
returns all row results, including errors. Callers decide whether to accept
successful rows or treat their whole local batch as atomic.

Remote environments keep **full private checkpoints in the trusted controller**.
They do not create database matches. The server validates each supplied
checkpoint and returns a separately projected frame. Never give checkpoints
to a model, opponent, spectator, or benchmark participant. Policies receive
only their seat-visible observation and legal mask.

A remote transition validates the returned checkpoint, rewards, catalog, mask
and tensor, then fetches all player frames before updating local state.
Transport errors or a failure fetching any frame preserve the previous state.
Gym also rolls back the entire learner/opponent exchange, including its RNG.
Clones hold independent checkpoints and share connection configuration.

Use native environments for peak throughput: remote stepping serializes full
checkpoints and incurs HTTP round trips. Each environment controller is intended
for one caller thread. `close()` has no server environment handle to release.
Arrays and checkpoint dictionaries returned to callers are independent copies.

The client sends an optional key only as a Bearer header, follows no redirects,
performs no implicit retries, and caps requests at 4 MiB and responses at 8 MiB.
`RemoteError.code` and `status` expose stable failures without response bodies,
credentials or endpoint URLs. Timeout is a socket timeout, not a total run
deadline. The current server is local-only and does not validate API keys;
do not expose it through a public proxy. Use TLS for future authenticated
remote deployments. Server and SDK engine versions must be compatible.
