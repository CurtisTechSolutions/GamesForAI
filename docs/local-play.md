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
scheduling, game briefings, simulation, forks, public authentication, PostgreSQL,
and OpenAPI documentation remain separate M1 increments.
