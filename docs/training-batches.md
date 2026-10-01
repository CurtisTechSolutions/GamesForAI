# Stateless training over REST

`POST /v1/batch/step` executes 1–64 independent operations in input order.
This endpoint is for the **owner of a private training environment**. It never
reads a persisted match, uses a match ID, updates ratings or stores episodes.
The current host exposes it only through the existing guarded loopback API.

```json
{
  "operations": [
    {"op": "create", "game_id": "connect4", "config": {}, "seed": 42, "seat": 0},
    {"op": "create", "game_id": "tictactoe", "seed": 43, "seat": 1}
  ]
}
```

Each result is either `{status: "ok", checkpoint, frame}` or
`{status: "error", error}`. The frame contains the selected seat's observation,
legal actions and mask, current actors, per-seat returns and reward deltas,
and distinct termination/truncation flags. Non-step operations have zero reward.

A checkpoint contains **full private state and RNG**. Keep it with the trusted
training controller; give model policies only their separate seat-visible
observation and legal-action mask. Checkpoints are not credentials or ranked
match handles. No operation can load a match, reveal another training caller's
state, or resume a benchmark from an ID.

Subsequent operations:
- `{op: "step", checkpoint, seat: 0, action: 3}` applies one discrete action.
- `{op: "observe", checkpoint, seat: 1}` validates and projects another seat.
  Omit seat for public spectator text with an empty catalog and all-false mask.
- `{op: "reset", checkpoint, seed: 44, position: "...", seat: 0}` starts an
  episode with the checkpoint's game/configuration. Omit position for the
  standard start. Create also accepts optional position notation.

Every checkpoint is validated for game, engine version, configuration and state
invariants. Invalid rows do not affect other rows. Successful calls are
deterministic for the same inputs, and retries cannot duplicate a database
write. Branch by submitting the same checkpoint with different actions.

The adapter permits four concurrent CPU workers and returns `ENGINE_BUSY`
instead of queuing unbounded work. Requests are limited to 4 MiB, serialized
rows to 1 MiB and responses to 8 MiB. Oversized results become row errors.
Input/body or batch-count errors reject the whole request; valid requests return
HTTP 200 with all row results, including errors. Engine events are deliberately
excluded from policy frames; they may contain private event payloads.

The endpoint transports and validates checkpoints each time. Use native
VectorEnv for maximum local throughput. Batch calls do not imply a cross-row
transaction; a client can commit its new checkpoints only after every requested
row succeeds. The full schema is generated in the local OpenAPI document.
