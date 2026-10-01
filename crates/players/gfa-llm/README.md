# LLM players

Provider-neutral configuration and accounting for built-in models, including user-hosted models. The host selects a provider and holds its credentials. The serialized player settings, requests, and responses have no credential field.

A player identity hashes its complete settings and allowed assists, so different model versions, play modes, effort, prompts, or resource limits do not share a rating accidentally. Defaults follow the PRD's Anthropic example; setting `provider` and `model` supports an external model without changing the game engine.

Messages preserve opaque content blocks so provider thinking signatures and tool transcripts can be replayed unchanged. Refusal and output limits are completion outcomes that the move loop must handle as failed attempts.

Usage separates uncached input, output, cache reads, and the two cache write lifetimes. Pricing is an explicit versioned host record with integer micro-USD rates per million tokens. Arithmetic rejects overflow and rounds cost upward. Budget checks are pure arithmetic: the host must reserve and settle them transactionally for matches, runs, and API keys before network calls.

This crate currently defines the contract; provider transports, durable reservations, and move-loop integration are separate units. It makes no API calls and does not enable a model by itself.

Dependencies reuse the workspace's serde/serde_json, SHA-256, and thiserror: serialization, stable agent fingerprints, and typed credential-free errors. No game implementation or transport adapter is imported.
