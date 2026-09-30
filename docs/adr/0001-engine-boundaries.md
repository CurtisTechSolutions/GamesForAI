# ADR 0001: Pure typed engines behind a validated JSON boundary

Status: Accepted

Follow PRD §5.2: game crates contain rules, the registry selects implementations, and consumers depend on the core contract. The typed Game trait serves in-process training; DynGame provides JSON type erasure for domain services. Every imported JSON state is validated before use. Failed dynamic transitions return no mutated state.

Initial-state construction and position export return Result rather than the infallible signatures in the PRD sketch. Invalid configuration and serialization errors must remain recoverable, as required by CS-2.

The core exposes serialization/schema derives to game crates so their only production crate dependency is gfa-core. Viewer scope is explicit from the start; public, player, and omniscient access will be authorized by the service. Stochastic games keep version-stable RNG state in their serializable state. SeededRng must never be used for authentication tokens.

Generic conformance tests cannot prove hidden-information non-leakage. Imperfect-information plugins must add paired-state tests whose private data differ while an unauthorized viewer's observation remains identical.
