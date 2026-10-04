# Contributing

Run commands from the repository root. Use the stable Rust toolchain configured
in `rust-toolchain.toml`, Node.js 22, pnpm 10.6.5 (pinned in `package.json`),
and GNU Make. Python SDK development also needs Python 3.10 or newer. On Windows,
use a shell that provides Make and POSIX tools, or run the equivalent commands
in the Makefile directly.

## Start developing

```sh
make help
make setup
make serve
```

Open `http://127.0.0.1:8080/` for the browser app; the API reference is at
`/docs/`. `make serve` installs the locked browser dependencies and builds the
production app before starting the server. `make serve-postgres` does the same
with PostgreSQL.

For hot reload, use `make serve-api` in one terminal and `make dev` in another.
Open Vite’s URL (normally `http://127.0.0.1:5173/`). API-only mode needs no browser
build and redirects its root URL to the API reference.

```sh
make serve-api PORT=9090 DB_PATH=./development.sqlite
GFA_API_TARGET=http://127.0.0.1:9090 make dev
make serve CLI_ARGS='--stockfish /usr/games/stockfish'
make mcp
```

Stockfish requires the Linux sandbox prerequisites described in the project
README. `make mcp` keeps command echoing off so stdout remains the MCP protocol
stream. Pass optional local CLI flags with `CLI_ARGS`.

For PostgreSQL, set `GFA_DATABASE_URL` through your shell or secret manager, then
run `make serve-postgres`. Use `POSTGRES_ENV=YOUR_VARIABLE` to select a different
environment variable. The database URL is never a Make command-line argument.

## Build and verify

| Command | Work performed |
| --- | --- |
| `make build` | Rust workspace and production web assets |
| `make check` | Rust format, Clippy, dependency boundaries, all-feature tests; web types, lint, boundaries and format |
| `make test-browser` | Build the app and server, then run production and development browser tests |
| `make format` | Format Rust and browser sources |
| `make audit` | Run the existing cargo-deny policy (install cargo-deny first) |
| `make docs` | Build Rust API documentation |
| `make openapi` | Generate `target/openapi.json` |

`make check` does not install dependencies, run browser/Python/Stockfish/
PostgreSQL integration suites, or execute model-training jobs. CI continues
to run those specialized checks separately. `make test` is an alias for the
Rust workspace tests. `make help` lists the smaller targets, including
`build-server`, `check-web`, and `test-rust`.

SQLx uses the checked-in offline query metadata by default. Override with
`SQLX_OFFLINE=false` when explicitly checking queries against a prepared
database, following the SQLite/PostgreSQL CI scripts. Cargo and pnpm commands
preserve their lockfiles.

## Python SDK

Use a virtual environment so installation stays separate from system packages:

```sh
python3 -m venv .venv
. .venv/bin/activate
make setup-python
make test-python
make build-python
```

On Windows, activate the environment using the script for your shell and pass
`PYTHON=python` if needed. You can also select an interpreter explicitly with
`make setup-python PYTHON=.venv/bin/python`.

Setup builds and installs the native extension with test, dataset, and tournament
extras, plus the wheel builder. Repeat it after changing Rust or Python SDK code
before running `test-python`. Training extras remain opt-in; see
[training examples](examples/README.md). Wheels are written to `dist/`.

## Pull requests and versions

Keep each feature or fix in its own PR. Use Conventional Commit titles:
`feat:` for new functionality, `fix:` for bug fixes, and `build:`/`chore:`
for developer tooling. Mark incompatible public API changes with `!` and a
`BREAKING CHANGE:` explanation. Release Please uses these commits to determine
semantic releases; a tooling shortcut alone does not bump the public API version.

Follow the dependency rules and clean-code requirements in [the PRD](docs/PRD.md).
Run the checks relevant to the change and preserve passing CI before merging.
