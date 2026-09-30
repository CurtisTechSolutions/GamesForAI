# Local play

Run the API with a persistent SQLite file:

```sh
cargo run -p gfa-cli --locked -- serve --sqlite ./gfa.sqlite --port 8080
```

The parent directory must exist. The server binds to `127.0.0.1`, prints its URL,
and shuts down cleanly on Ctrl+C (or SIGTERM on Unix). Port `0` selects an available
port. This is single-user local mode: any local caller can control either seat.
Use the printed address directly; public proxy hosting and agent/seat credentials
are not part of this mode.

List games and create a match:

```sh
curl http://127.0.0.1:8080/v1/games
curl -X POST http://127.0.0.1:8080/v1/matches \
  -H 'Content-Type: application/json' \
  -d '{"game_id":"tictactoe","seed":42}'
```

Creation returns a `match_id`, an observation, and seat 0's legal actions. Replace
`MATCH_ID` below with that ID. Send the current `turn` with each move:

```sh
curl -X POST http://127.0.0.1:8080/v1/matches/MATCH_ID/actions \
  -H 'Content-Type: application/json' -H 'Idempotency-Key: first-move' \
  -d '{"seat":0,"turn":0,"action":"r2c2"}'
curl 'http://127.0.0.1:8080/v1/matches/MATCH_ID/state?seat=1'
curl 'http://127.0.0.1:8080/v1/matches/MATCH_ID/legal-actions?seat=1'
curl 'http://127.0.0.1:8080/v1/matches/MATCH_ID/replay?seat=0'
```

Moves return `accepted_action` and `state`. Retry the identical move with the same
key to receive its original response; reusing a key with changed input returns
HTTP 409. Stale turns also return 409 with current turn and legal actions.
Invalid moves return 422 and do not change the match. Errors consistently use
`{"error":{"code":...,"message":...,"hint":...,"details":...}}`.

Omit `seat` on state or replay to request a spectator view. Creation defaults to
seat 0; `POST /v1/matches?seat=1` selects the other seat's initial view.
`GET /v1/games/{game_id}` supplies configuration and action schemas.
Tic-Tac-Toe and Connect Four are registered. For Connect Four, an action can be
`"4"`, `{"column":4}`, or `{"index":3}`.

The SQLite file keeps events and retry receipts across restarts. Replays use the
recorded engine version and reject unavailable versions. Request bodies are
limited to 64 KiB. Browser calls must have the same origin; the future web client
can use a same-origin development proxy.

This first local host exposes the existing external-player lifecycle. Opponent
scheduling, simulation, forks, public authentication, PostgreSQL,
and OpenAPI documentation remain separate M1 increments.

## Live state

Connect a WebSocket to `ws://127.0.0.1:8080/v1/matches/MATCH_ID/stream?seat=0`.
Omit `seat` for a spectator projection. The first message contains the current
state; subsequent messages contain each newly committed turn in order:

```json
{"type":"state","state":{"match_id":"...","turn":1,"observation":{},"...":"..."}}
```

Submit moves through REST. Text/binary messages sent to the stream close it with
code 1003. Each connection has a bounded write timeout; reconnecting begins with
the current state, and REST replay provides earlier history. A terminal state is
sent before the stream closes. Service errors use `{"type":"error","error":...}`.

Commit notifications wake streams immediately. A two-second reconciliation pass
recovers changes from other processes or missed notifications. Catch-up reads
persisted replay and applies the selected viewer to every frame. Local access
rules also apply to upgrades; the host allows at most 64 concurrent streams and
closes streams before closing SQLite during shutdown. The initial protocol
streams state frames; paginated event delivery and resume cursors are later work.

## Model briefings

Read `GET /v1/games/tictactoe/info` (or `connect4`) before creating a match.
Use `?detail=compact` for rules without schemas or long examples, and
`?format=markdown` for a prompt-ready document. `config` accepts URL-encoded
JSON; `seat=0` or `seat=1` adds seat-specific notes.

JSON briefings contain `info_version`, `approx_tokens`, and an ordered
`sections` array. Each section has a stable `id`, explanatory `text`, and
structured `data`. Markdown renders those exact fields in the same order.
Examples run the installed engine at a synthetic standard start with seed 0;
they never access a live match. Copy live turn numbers when submitting moves.
The estimate is a byte-based heuristic, not a specific model's token count.

Game briefings return representation-specific ETags and support
`If-None-Match`, including weak tags and tag lists. They need no credentials
within local mode; the loopback and Host/Origin restrictions still apply.
Unavailable capabilities are stated explicitly in their sections.

 
## Match briefings

Creation includes a compact `info` briefing alongside the existing observation
and legal-action fields. Set `"include_info": false` in the JSON body to omit it.

Read `GET /v1/matches/MATCH_ID/info?seat=1&detail=compact` for that seat's
current briefing. The same `format=json|markdown` and `detail=compact|full`
options apply. Omit `seat` for a spectator view. Configuration comes from the
match and cannot be overridden by this query.

The first section, `match`, records the current status and turn, selected seat,
participants, starting position, untimed control, rejection policy, available
assists, task, recording policy, and viewer-scoped state. The later game
examples use a synthetic standard start, even when the match has a custom
start. Custom notation is exposed only for deterministic perfect-information
games (or an explicitly authorized in-process omniscient viewer); other viewers
receive their starting observation instead. Seeds and private engine events
are not included.

Match briefings are not cached. The service prepares creation briefings before
committing the match, and reads subsequent briefings from one consistent event
snapshot. In-process callers that only need state can continue using
`create_match`; transports use `create_match_with_info`.

 
## Standalone positions

Validate a complete position without creating a match:
`POST /v1/games/tictactoe/positions/validate?seat=0` with
`{"start":{"position":"<game notation>"},"seed":42}`.
Alternatively use `start: {"state": ...}` with a structured state.
Optional `config` uses the same options as match creation.

The response includes canonical notation, structured state, observation/tensor,
legal actions and mask, actors, returns, and terminal status. Finished positions
are valid for analysis but cannot start a match. Imported states receive a new
seed for future randomness; supply one to reproduce a validation, or reuse the
returned seed. This endpoint accepts caller-supplied positions and never loads
private state from an existing match.

 
## API contract

`GET /v1/openapi.json` serves OpenAPI 3.1 generated from the Rust request and
response types. It lists installed routes, query/header parameters, JSON and
Markdown briefing formats, and error envelopes. Routers without live updates
omit the WebSocket path. Export instructions are in `schemas/README.md`.

## Match history

`GET /v1/matches/{id}` returns current public metadata reconstructed from the event log. `GET /v1/matches?game_id=tictactoe&status=finished&limit=50` scans local history in ascending match-id order. Pass the response's `next` as `after` to continue. The limit bounds records scanned before filtering, so an empty filtered page can still have a continuation. A new scan includes concurrent creations that sort before your current cursor.

Metadata omits seeds, starting state, observations and reasoning. This index is for the loopback-restricted single-user mode; public multi-user hosting needs owner/visibility authorization.

## Interactive API documentation

Open [the local API explorer](http://127.0.0.1:8080/docs/) after starting the server. The explorer uses the generated OpenAPI contract and bundles its JavaScript and styles with the server, so it works without a CDN. Expand an operation, edit the example, and select **Execute**. Start with `POST /v1/matches`, copy its `match_id`, then submit a move through `POST /v1/matches/{id}/actions` with the current turn and seat.

Requests run against the same local server. The explorer uses the same Host and Origin checks as the API, and no external schema validator is contacted. Browser tests create a match and make a move through the visible controls.

To run the browser check locally, build the CLI with `cargo build -p gfa-cli`, install Chromium with `pnpm exec playwright install chromium`, then run `pnpm test:browser`. CI uses the Chrome installation supplied by the GitHub runner image.
