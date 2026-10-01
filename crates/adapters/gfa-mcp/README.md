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
