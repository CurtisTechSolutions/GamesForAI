# GamesForAI

A game library that serves as a training and evaluation environment for AI systems. It has Rust game engines behind one standardized API (REST, WebSocket and MCP), built-in opponents such as Stockfish, persistent match history, and a React + Phaser frontend for playing games, watching live matches and viewing replays.

See the [Product Requirements Document](docs/PRD.md).

## Run locally

Use `make help` for setup, run, build, and verification shortcuts. The
[contributor guide](CONTRIBUTING.md) documents prerequisites and optional Python
setup. `make setup`, `make serve`, and `make dev` (in a second terminal) start the
local development environment.


```sh
cargo run -p gfa-cli --locked -- serve --sqlite ./gfa.sqlite --port 8080
```

Open `http://127.0.0.1:8080/docs/` for the interactive API reference. Local mode
binds to loopback and treats the local caller as the match owner.

For PostgreSQL, enable the backend and put the connection URL in an environment
variable so credentials do not appear in the command arguments:

```sh
# Set GFA_DATABASE_URL through your shell or secret manager first.
cargo run -p gfa-cli --features postgres --locked -- serve --postgres-env GFA_DATABASE_URL
```

Use a dedicated database; startup applies embedded migrations. For remote
PostgreSQL, require server certificate verification with `sslmode=verify-full`.
SQLite is the default. The storage flags are mutually exclusive. Both backends
persist moves and idempotency receipts atomically and recover replays after a
graceful shutdown and restart.

## Opponents and analysis

`GET /v1/games/{game_id}/opponents` lists installed players. Random is available
for every game. Deterministic, sequential games with perfect information also
have MCTS; two-player games have alpha-beta minimax. Search levels are resource
budgets until calibration results are published; the API reports null ratings.

`POST /v1/analysis` accepts `game_id`, `from` (a match/seat/optional turn, or a
standalone position/state), `opponent` (id, optional level and limits), and an
independent `seed`. It returns legal recommendations, evaluation, principal
variation, node count, depth and whether the budget was exhausted. Match analysis
requires `assists.allow_analysis: true`. A standalone copy of an active position
cannot bypass that policy. Searches run in four bounded workers and receive
only the authorized observation and legal actions.

Sudoku also advertises the `reference` opponent. Analysis returns its next
placement and supporting candidate eliminations. Search-backed steps are marked
`advice.is_guess: true`; an incorrect entered digit produces an explicit
`solution_check` erasure recommendation. Ordinary state responses do not include
these recommendations or the solution grid.

## Play against a built-in opponent

Create an interactive match with one assignment per player:

```json
{"game_id":"tictactoe","seats":[{"type":"self"},{"type":"opponent","opponent":{"id":"minimax","level":3},"seed":19}]}
```

An omitted list keeps both seats external. A bot in the starting seat plays before
creation returns; use `?seat=1` when joining the second seat. An action request
returns `opponent_actions` and the final observation after those replies. The
caller's move, automatic replies, and idempotency receipt commit together, so
worker failure or a conflicting write leaves the attempted batch unapplied.

Planning seeds are materialized independently from game randomness. Replay
reapplies recorded actions without rerunning opponents. Forks may replace
`seats`; omitted assignments retain the parent's configuration. All-opponent matches return their initial state immediately and run in the
background. The server scans persisted matches at startup and between batches,
so a restart resumes pending turns. Live streams receive each committed update.
Four concurrent scheduler tasks and four search workers bound resource use.
Interactive reply chains longer than eight moves continue through this runner.

`GET /v1/matches/{id}/events?since=&limit=&seat=` pages through consecutive
event sequences from one recorded revision. Omit `since` for the beginning;
use `next` for the following page. Limits are 1–100. The service projects each
event to the authorized player or spectator: private actions, reasoning, chance
events, match seeds, and internal fork permissions are omitted when unauthorized.
Own reasoning is available immediately; other reasoning in perfect-information
games becomes available after completion.

Full replay bundles include the game/version, normalized config, event revision,
projected events (including reasoning and controls), and reconstructed per-turn
observations. Authorized in-process omniscient exports also include the initial
engine state and RNG seed, so custom starts and forked histories can be rebuilt
exactly. The ordinary REST views omit those private inputs. Full responses have
a 16 MiB budget; larger histories remain available through paginated events.

Game briefings provide compact rules and full schemas/examples. Full previews mark
large numeric arrays in `omitted_fields`; fetch the normal state/legal-action
routes for complete tensors and masks. This keeps model prompts readable without
changing the live state contract.

## Chess

Chess is enabled by default and uses the same create, move, simulate, fork,
analysis and replay routes. Submit UCI moves such as `e2e4`, `e1g1` or `a7a8n`,
or the structured/index representation from `legal_actions`. White is seat 0.
`GET /v1/games/chess/info` explains draw claims and every encoding.

Use `start: {"position":"<six-field FEN>"}` for a custom board. A bare FEN begins
new repetition history; complete played-state JSON keeps the original position
and actions. Replay and forks retain that history. The engine accepts standard
chess only. Generic random/search opponents are available; Stockfish integration
is tracked separately.


To enable a local Stockfish installation on Linux:

`cargo run -p gfa-cli -- serve --stockfish /usr/games/stockfish`

Install Stockfish, bubblewrap and util-linux, and allow bubblewrap to create user
namespaces under the host's security policy. Startup verifies a sandboxed search
before opening the database. The application never disables host sandbox policy.
Hosts may configure the pool, CPU/address-space limits, threads and hash size
through StockfishConfig. Engine paths and resource policy cannot be supplied by
match participants.

Chess then advertises stockfish in its opponent catalog. Select it for an
opponent seat or POST /v1/analysis with, for example,
{"id":"stockfish","level":3}, or
{"id":"stockfish","uci":{"elo":1500,"multipv":3}} as the opponent.
Explicit level, skill and Elo strength choices are mutually exclusive. Installed
engine capability bounds are authoritative. Skill presets use levels 1–10 but
remain uncalibrated, with null ratings, until a measured calibration is published.
Stockfish's weaker settings may randomize play; a planning seed does not promise
deterministic engine strength-limited decisions. Idempotent action retries reuse
the persisted result. Analysis exposes typed centipawn/mate scores and bounded,
rules-validated variations in advice.details.
