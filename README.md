# GamesForAI

A game library that serves as a training and evaluation environment for AI systems. It has Rust game engines behind one standardized API (REST, WebSocket and MCP), built-in opponents such as Stockfish, persistent match history, and a React + Phaser frontend for playing games, watching live matches and viewing replays.

See the [Product Requirements Document](docs/PRD.md).

## Run locally

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
