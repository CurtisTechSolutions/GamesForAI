# Built-in players

Players receive a seat's observation and legal actions, with injected monotonic time and explicit node/depth/seed limits. `Random` works for every legal-action list. `SearchOpponent` reconstructs a private planning state through the engine's observation hook, without accepting the live match state.

- Minimax uses iterative deepening and alpha-beta pruning for two-player zero-sum games. It returns the last completed iteration if its budget expires.
- MCTS uses UCT selection, seeded random expansion and rollouts, and per-seat normalized reward backup.
- Both enforce a node budget and check time before every node. With a fixed clock and seed their choices and diagnostics are reproducible. Time-limited results may vary across hosts.

The initial search adapters support deterministic sequential perfect-information games. Engines without observation reconstruction fail closed. Hidden-information and stochastic engines need sampling policies before search can be enabled. Level 1–10 currently maps to resource limits; these levels are provisional until measured calibration is published. Nonterminal evaluation is the engine's current return, so no game-specific heuristic is assumed.
