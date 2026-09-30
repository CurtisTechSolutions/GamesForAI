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
