# SQLite match persistence

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
This adapter stores the initial match event model; searchable metadata,
retention policies, snapshots, and PostgreSQL support are separate increments.
