# Match persistence

`SqliteMatchStore` implements the storage port in `gfa-service`. Inject an
`Arc<SqliteMatchStore>` into the lifecycle service when wiring a host.
`open(path).await` creates a file when needed and applies embedded migrations.
The parent directory must already exist. `in_memory().await` creates an isolated
ephemeral store. Call `close().await` during host shutdown.

Events and idempotency receipts commit together. A SQLite immediate transaction
reserves the writer before checking the expected event revision. Concurrent retries
with the same key and request return the original response; a reused key with
different input fails. Reads use a transaction so revision, events, and receipts
come from one snapshot. File databases use WAL and full synchronous durability.

Production queries use SQLx compile-time checks. Committed `.sqlx` metadata lets
`cargo test --workspace --all-features --locked` build without a development
database. CI also compiles against a fresh schema built from the migrations and
checks the generated metadata against the committed files.

After changing a query or migration, from the repository root:

```sh
export GFA_SQLX_WORK_DIR="$(mktemp -d)"
python3 .github/scripts/sqlite-query-check.py prepare
cargo clean -p gfa-store
DATABASE_URL="sqlite:$GFA_SQLX_WORK_DIR/schema.sqlite" \
SQLX_OFFLINE_DIR="$GFA_SQLX_WORK_DIR/queries" \
cargo check -p gfa-store --locked
cp "$GFA_SQLX_WORK_DIR"/queries/query-*.json .sqlx/
python3 .github/scripts/sqlite-query-check.py verify
```

When a query is removed, also remove its obsolete `.sqlx/query-*.json` file.
## PostgreSQL

Enable the `postgres` feature and connect with
`PostgresMatchStore::connect(url).await`. The pool applies the embedded
`migrations/postgres` migrations at startup. Use a dedicated database and include
`sslmode=verify-full` for a remote database. Call `close().await` on shutdown.

PostgreSQL appends lock the match row before checking receipts and revisions.
Snapshot reads use a repeatable-read transaction; assist counters use atomic
upserts. Match IDs have C collation so cursor ordering agrees with the service.
The SQLite and PostgreSQL adapters implement the same storage port.

The PostgreSQL CI job compiles queries against a fresh migration-derived schema,
checks the committed PostgreSQL metadata, and runs the ignored integration test
against a separate empty database. To run it locally, set `DATABASE_URL` to a
migrated query-checking database and `GFA_TEST_POSTGRES_URL` to an isolated empty
test database, then run:

```sh
cargo test -p gfa-store --no-default-features --features postgres --locked -- --ignored
```

Retention policies and materialized replay snapshots are separate increments.


## LLM call journal and spending accounts

Both backends implement `gfa_service::llm_ledger::LlmLedger`. A call reservation writes the complete credential-free request and reserves a conservative token/cost bound against its match and provider-key accounts, plus an optional benchmark-run account. Account references are host-generated names such as `key:primary-anthropic`; never use an API key as a reference. Limits are immutable for an account's lifetime, so a new budget period uses a new reference.

Only `ReserveResult::Reserved` authorizes a new HTTP call. An existing call ID returns its stored response/status and never authorizes retransmission. Reusing an ID with different input, or using a different ID for the same match/seat/turn/attempt, fails. SQLite reserves its writer before checking balances. PostgreSQL inserts/locks account rows in a fixed order; every call, reservation, and settlement shares those same locks. All affected balances and the journal update commit together.

Timeouts and cancelled calls keep their full reservation as `Uncertain`. The host may reconcile a known response later. Settlement computes cost from the immutable pricing snapshot, separates all token classes, and is idempotent for an identical provider response. If reported usage exceeds the host's reservation, the actual amount is recorded and `exceeded_reservation` is true. Exhausted accounts reject subsequent reservations; the host must surface the estimation failure rather than reporting the call as free or silently releasing funds.

Transcripts are paged by `(turn, attempt)` within a host-authorized match/seat. Pages contain at most 100 records and ordinarily at most 8 MiB of stored payload; a larger single record is returned whole. Continue from the last returned call until an empty page. These trusted persistence methods do not expose an HTTP endpoint, hold provider keys, or enable model seats on their own.

Integration tests exercise concurrent reservations/settlements, duplicate dispatch attempts, rollback across scopes, unknown billing, restart recovery, actual usage above a reservation, and transcript ordering against SQLite and PostgreSQL.
