# Implementation status and local handoff

The authorized target remains the full **M0–M7** roadmap in PRD v0.4. Branch: `feat/gamesforai`. Draft PR: https://github.com/CurtisTechSolutions/GamesForAI/pull/2.

## Implemented

- Pure typed Game contract, validated JSON DynGame adapter, explicit player/spectator/omniscient views, structured errors, generated schemas, serializable seeded RNG, action masks, and feature-gated game registry.
- Tic-Tac-Toe rules, canonical actions, validated imports, terminal returns, and observations. Exhaustive tests cover all 5,478 reachable boards, alongside conformance and failure atomicity tests.
- Connect Four bitboard engine, gravity checks and reverse-history validation that rejects unreachable positions. Conformance, geometry, unreachable-position, and terminal-win tests.
- Cargo and pnpm workspaces, Rust formatting/Clippy, strict TypeScript, ESLint, Prettier, dependency direction checks, cargo-deny, initial core compatibility checks, and a release-mode benchmark.
- React/Vite development shell, API-client boundary, shared UI component, Phaser plugin host and accessible text controls. There is no playable server-backed UI yet.

## Verification

The first foundation CI runs passed the Rust tests, Clippy, architecture checks, the empty registry build, web type checking, web dependency rules, and the Vite production build. License/advisory checks passed after adding explicit internal dependency versions and the MIT-0 license used by a transitive test dependency.

The first release benchmark measured **9,857,331 Tic-Tac-Toe steps/s** on a GitHub Ubuntu runner (761,931 steps, one sample). This exceeds the PRD's 5M target for that run, not a guarantee on other hardware.

The latest commit still needs all checks green after Connect Four and formatter fixes. Do not assume unfinished features or unrun tests work.

## Resume locally

The user chose to reopen GamesForAI as a local Codex project because this chat's shell cannot start and its Windows Node tool rejects the WSL workspace URI. GamesForAI is not currently listed among saved local projects.

Clone or fetch the repository and check out `feat/gamesforai`, then open that directory as a Codex project using a runtime whose paths match the checkout. Read the PRD and this file first. Keep branch names under `feat/`, `fix/`, etc.; do not use `codex/`.

Continue in dependency order:

1. Finish any outstanding foundation CI failures and commit the generated Rust lockfile; use locked dependency installs.
2. M1: shared API types, transport-independent service/ports, event-sourced SQLite/Postgres persistence, race-safe/idempotent actions, authorization, info routes, REST/WebSocket, Sudoku, opponents, simulation, custom starts and forks.
3. M2: official rmcp stdio/HTTP integration, chess, UCI/Stockfish, LLM providers and budget/accounting controls. Real model evaluations need configured credentials and an agreed spending budget.
4. M3: game scenes, play, live viewing, replay/forking and inspector pages.
5. M4: in-process PyO3 training API, wrappers, vector stepping, datasets, ratings and tournaments.
6. M5: immutable suites/position sets, public and hidden splits, runners, reports and leaderboards. Do not invent benchmark reports or calibrated ratings.
7. M6–M7: remaining game catalog and GTP engines, then imperfect-information games, poker rules/duplicate evaluation/equity/CFR policies, and information-leakage tests.

The PR is intentionally a draft; none of M1–M7 is complete. No deployment has been performed.
