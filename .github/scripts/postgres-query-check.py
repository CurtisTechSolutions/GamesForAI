"""Verify checked-in SQLx PostgreSQL metadata against the migrated CI database."""
import json
import os
from pathlib import Path


def metadata(directory):
    values = {}
    for path in directory.glob("query-*.json"):
        value = json.loads(path.read_text())
        if value["db_name"] == "PostgreSQL":
            values[path.name] = value
    return values


generated = metadata(Path(os.environ["SQLX_OFFLINE_DIR"]))
if not generated or generated != metadata(Path(".sqlx")):
    raise SystemExit("PostgreSQL SQLx metadata is missing or stale.")
