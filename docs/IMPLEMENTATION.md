# Implementation status

Target: the full M0–M7 roadmap in PRD v0.4. This document records implemented and verified behavior; it does not redefine the requested scope.

## Current build

- M0: typed pure engine contract, JSON type erasure, explicit viewer scope, serializable seeded RNG, feature-gated registry, Tic-Tac-Toe, conformance suite, exhaustive reachable-board validation, and dependency-direction checker implemented. CI validation pending.
- M0 still requires frontend workspace/lint setup, dependency/license checks, performance measurements, and core API compatibility checks.
- M1–M7: not implemented yet.

The implementation starts from the repository's documentation-only main branch. No milestone is considered complete until its PRD exit criteria are met.

## Execution environment

The initiating Codex session could read and write GitHub but its local shell failed to start and Node rejected the workspace URI before executing code. GitHub Actions is being used to establish whether compilation and tests can run remotely. No local compilation or performance result has been claimed.
