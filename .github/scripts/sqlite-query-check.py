"""Prepare a migration-derived schema and verify SQLx offline query metadata."""
import json
import os
from pathlib import Path
import sqlite3
import sys

work = Path(os.environ["GFA_SQLX_WORK_DIR"])
database = work / "schema.sqlite"
queries = work / "queries"

if sys.argv[1] == "prepare":
    queries.mkdir(parents=True, exist_ok=True)
    database.unlink(missing_ok=True)
    for path in queries.glob("query-*.json"):
        path.unlink()
    with sqlite3.connect(database) as connection:
        for migration in sorted(Path("crates/adapters/gfa-store/migrations/sqlite").glob("*.sql")):
            connection.executescript(migration.read_text())
elif sys.argv[1] == "verify":
    def metadata(directory):
        return {
            path.name: json.loads(path.read_text())
            for path in directory.glob("query-*.json")
            if json.loads(path.read_text())["db_name"] == "SQLite"
        }

    generated = metadata(queries)
    if not generated or generated != metadata(Path(".sqlx")):
        raise SystemExit("SQLx metadata is missing or stale; regenerate it from the migrations.")
else:
    raise SystemExit("usage: sqlite-query-check.py prepare|verify")
