# GamesForAI

A game library for training and evaluating AI systems, specified in the [Product Requirements Document](docs/PRD.md).

The full target is milestones **M0–M7**. This branch currently contains the engine foundation and the first two games. The API, persistence, MCP, additional games, training SDK, evaluation platform, and playable UI are still to be built. See [implementation status](docs/IMPLEMENTATION.md).

## Workspaces

- Rust: pure game contract and JSON adapter, seeded RNG, feature-gated registry, Tic-Tac-Toe, Connect Four, shared conformance tests, and architecture checks.
- Web: React 19/Vite application shell, shared UI, API client boundary, Phaser scene host, and accessible text controls. The shell is not yet connected to a game server.

## Development

Install stable Rust, Node.js 22, and pnpm 10.6.5.

```sh
cargo test --workspace --all-features
cargo xtask check-deps
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo bench -p gfa-game-tictactoe --bench playout
pnpm install --frozen-lockfile
pnpm typecheck
pnpm lint
pnpm build
pnpm dev
```

CI also runs cargo-deny, checks the empty game-registry feature set, and checks core API compatibility when a baseline exists.

Contributions use Conventional Commit messages and branch prefixes such as `feat/` and `fix/`. Production game crates depend only on `gfa-core`; game selection belongs in `gfa-games`. The frontend renders authoritative server data and uses `api-client` for network access.

Licensed under GPL-3.0-only.
