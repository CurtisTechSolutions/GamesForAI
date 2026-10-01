# MCP adapter

This crate uses the official Rust SDK, rmcp 3.5, and calls gfa-service directly.
It contains no concrete game, storage, HTTP-adapter or host imports. Hosts inject
the service and an authorized viewer; omniscient MCP access is refused.

The first tools are list_games, get_game_info and list_opponents. Tool parameters
are flat, descriptions contain executable examples, and every successful or
recoverable failed result has readable text plus structuredContent matching
its declared outputSchema. Argument/result sizes are bounded. Unknown tools are
protocol errors; domain and input errors are ordinary isError tool results.

MCP DTOs use the SDK's JSON Schema 2020-12 generator. The existing game-plugin
schema dependency remains unchanged. Official-client protocol tests and output
schema validation run at the application layer, where concrete games may be
named. Stdio/HTTP composition and playing tools are separate implementation units.

Dependency justification: rmcp supplies maintained protocol negotiation, wire
types and transports rather than a project-specific JSON-RPC implementation.
Reference: https://github.com/modelcontextprotocol/rust-sdk


Match controls: `create_match`, `get_state`, `get_legal_actions`, `make_move`, and `resign` use the host-authorized viewer. A spectator can read public state but cannot create or control a player. `play_as` must agree with the host-selected player seat. This local composition is not multiuser authorization; a hosted deployment needs a match-access policy before granting a viewer.

States contain the engine text board and sorted canonical action strings, without tensors, action masks, or duplicate JSON boards. Move errors retain legal actions and recovery context. Supply both `turn` and `idempotency_key` for safe retries; without an explicit turn, the adapter reads the current turn and the service still rejects concurrent changes. Opponent replies and the resulting state arrive in the same move result. Reasoning is stored through the normal service command.


`simulate_moves` returns compact final states for independent lines and the first illegal action in each line. `analyze_position` returns ranked recommendations with labeled provider advice and expected game returns kept separate from engine scores. Both use the host-selected seat, honor the match's stored assist permissions, and support historical turns without changing live state. Simulation usage is durably accounted by the service. Optional analysis budgets go through the same validated worker limits as REST.
