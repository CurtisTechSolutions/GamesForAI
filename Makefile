SHELL := /bin/sh
.DEFAULT_GOAL := help

CARGO ?= cargo
PNPM ?= pnpm
PYTHON ?= python3
DB_PATH ?= ./gfa.sqlite
PORT ?= 8080
POSTGRES_ENV ?= GFA_DATABASE_URL
CLI_ARGS ?=
SQLX_OFFLINE ?= true
export SQLX_OFFLINE

.PHONY: help setup setup-rust setup-web setup-python serve serve-api serve-postgres mcp dev \
	build build-rust build-server build-web build-python check check-rust check-web \
	test test-rust test-browser test-python lint-rust check-deps audit format \
	format-rust format-web format-check format-check-rust format-check-web \
	typecheck-web lint-web docs openapi

help: ## Show commands and configurable defaults.
	@printf '%s\n' 'GamesForAI developer commands' ''
	@awk 'BEGIN {FS = ":.*## "} /^[a-z][a-z-]*:.*## / {printf "  %-22s %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf '\n%s\n' 'Overrides: CARGO, PNPM, PYTHON, DB_PATH, PORT, POSTGRES_ENV, CLI_ARGS, SQLX_OFFLINE'
	@printf '%s\n' 'Example: make serve PORT=9090 DB_PATH=./development.sqlite'

setup: setup-rust setup-web ## Fetch locked Rust and browser dependencies.

setup-rust: ## Fetch Rust dependencies without changing Cargo.lock.
	$(CARGO) fetch --locked

setup-web: ## Install the pinned browser dependencies.
	$(PNPM) install --frozen-lockfile

setup-python: ## Install the native SDK and test/dataset/tournament extras (use a virtualenv).
	$(PYTHON) -m pip install '.[test,datasets,tournaments]' 'maturin>=1.9,<2'

serve: build-web ## Build and serve the browser app + SQLite API at http://127.0.0.1:8080/.
	$(CARGO) run -p gfa-cli --locked -- serve --web-dir web/apps/site/dist --sqlite "$(DB_PATH)" --port "$(PORT)" $(CLI_ARGS)

serve-api: ## Run only the SQLite API (pair with make dev for hot reload).
	$(CARGO) run -p gfa-cli --locked -- serve --sqlite "$(DB_PATH)" --port "$(PORT)" $(CLI_ARGS)

serve-postgres: build-web ## Serve the app + API using a PostgreSQL URL from POSTGRES_ENV.
	$(CARGO) run -p gfa-cli --features postgres --locked -- serve --web-dir web/apps/site/dist --postgres-env "$(POSTGRES_ENV)" --port "$(PORT)" $(CLI_ARGS)

mcp: ## Run the local MCP server over stdin/stdout.
	@$(CARGO) run -p gfa-cli --locked -- mcp --sqlite "$(DB_PATH)" $(CLI_ARGS)

dev: ## Run the browser with hot reload (run make serve-api in another terminal).
	$(PNPM) dev

build: build-rust build-web ## Build the Rust workspace and production browser assets.

build-rust: ## Build all Rust workspace members.
	$(CARGO) build --workspace --locked

build-server: ## Build the local gfa CLI/server.
	$(CARGO) build -p gfa-cli --locked

build-web: setup-web ## Install locked dependencies and build the browser application.
	$(PNPM) build

build-python: ## Build a release Python wheel into dist/.
	$(PYTHON) -m maturin build --release --locked --out dist

check: check-rust check-web ## Run Rust and web checks; browser/Python integrations are separate.

check-rust: format-check-rust lint-rust check-deps test-rust ## Check Rust formatting, lints, architecture and tests.

check-web: typecheck-web lint-web format-check-web ## Check browser types, lints, architecture and formatting.

test: test-rust ## Run workspace unit/integration tests (alias for test-rust).

test-rust: ## Test the Rust workspace with all features and the locked dependency graph.
	$(CARGO) test --workspace --all-features --locked

test-browser: build-server build-web ## Test the production app and dev browser against a real backend.
	$(PNPM) test:browser

test-python: ## Test the installed native Python SDK (run setup-python after Rust changes).
	$(PYTHON) -m pytest python/tests -q

lint-rust: ## Run Clippy on every target/feature, treating warnings as errors.
	$(CARGO) clippy --workspace --all-targets --all-features --locked -- -D warnings

check-deps: ## Enforce Rust layer boundaries and the minimal game registry build.
	$(CARGO) xtask check-deps
	$(CARGO) check -p gfa-games --no-default-features --locked

audit: ## Check dependency policy; requires cargo-deny.
	$(CARGO) deny --locked check

format: format-rust format-web ## Format Rust and browser source files.

format-rust: ## Apply rustfmt to the Rust workspace.
	$(CARGO) fmt --all

format-web: ## Apply the browser's Prettier configuration.
	$(PNPM) format

format-check: format-check-rust format-check-web ## Check formatting without changing source files.

format-check-rust: ## Verify Rust formatting.
	$(CARGO) fmt --all -- --check

format-check-web: ## Verify browser formatting.
	$(PNPM) format:check

typecheck-web: ## Check the strict TypeScript project.
	$(PNPM) typecheck

lint-web: ## Check browser lint rules and package dependency boundaries.
	$(PNPM) lint

docs: ## Build Rust API documentation without dependency documentation.
	$(CARGO) doc --workspace --no-deps --locked

openapi: ## Write the generated OpenAPI schema to target/openapi.json.
	@mkdir -p target
	$(CARGO) run -p gfa-http --example export-openapi --locked > target/openapi.json
