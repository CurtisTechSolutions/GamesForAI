# LLM players

Provider-neutral configuration and accounting for built-in models, plus an Anthropic Messages HTTP transport. Other providers, including user-hosted models, can implement the same trait. The host selects a provider and holds its credentials. The serialized player settings, requests, and responses have no credential field.

A player identity hashes its complete settings and allowed assists, so different model versions, play modes, effort, prompts, or resource limits do not share a rating accidentally. Defaults follow the PRD's Anthropic example; setting `provider` and `model` supports an external model without changing the game engine.

Messages preserve opaque content blocks so provider thinking signatures and tool transcripts can be replayed unchanged. Refusal and output limits are completion outcomes that the move loop must handle as failed attempts.

Usage separates uncached input, output, cache reads, and the two cache write lifetimes. Pricing is an explicit versioned host record with integer micro-USD rates per million tokens. Arithmetic rejects overflow and rounds cost upward. Budget checks are pure arithmetic: the host must reserve and settle them transactionally for matches, runs, and API keys before network calls.

`AnthropicProvider::new(api_key)` creates a client for the fixed HTTPS Messages endpoint. The key remains in a sensitive header and is excluded from debug output, serializable configuration, and sanitized errors. Constructing a client makes no API calls. Calling `LlmProvider::complete` sends one request; the host must reserve budget first. Durable reservations and match move-loop integration are separate units, so adding this crate alone does not enable an opponent.

The transport sends explicit effort and defaults to adaptive thinking with a returned summary. Models without these capabilities can use `thinking: "disabled"` or `context_editing: false`; both settings contribute to agent identity. Unsupported model/settings combinations are returned as configuration errors, with no silent fallback. Direct mode uses the action/reasoning JSON schema. Tools mode uses strict tool definitions, automatic tool choice, and server-side clearing of older tool results. The immutable system briefing has a five-minute prompt-cache breakpoint; tool definitions precede it. The local history and opaque thinking signatures remain unchanged.

Requests and responses are capped at 4 MiB. The configured timeout covers the network request and response body. Redirects, ambient proxies, and automatic retries are disabled. A cancellation, timeout, or malformed response can leave provider billing unknown; hosts must retain the pending reservation conservatively and reconcile it, rather than treating the call as free. Refusal, output exhaustion, and other provider stop reasons remain ordinary completions with usage. The game loop decides whether they yield a valid action.

Unit and loopback HTTP tests cover both wire formats, signed history, cache accounting, response limits, timeout/cancellation, redirection, sanitized errors, and the absence of hidden retries. They use test credentials and make no paid provider calls. Live model compatibility requires a separately configured integration run.

Protocol references: [thinking](https://platform.claude.com/docs/en/build-with-claude/thinking), [structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs), [prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching), and [context editing](https://platform.claude.com/docs/en/build-with-claude/context-editing).

Dependencies reuse serde/serde_json, SHA-256, and thiserror for serialization, stable agent fingerprints, and typed errors. Reqwest with Rustls supplies asynchronous HTTPS without a system OpenSSL dependency; Axum, Tokio, and futures-util are test-only dependencies for loopback fixtures. No game implementation or application transport adapter is imported.
