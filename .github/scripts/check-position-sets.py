"""Reject edits to previously published dataset versions on pull requests."""
import os
import pathlib
import subprocess

base = os.environ.get("POSITION_BASE_SHA")
if base:
    paths = subprocess.check_output(
        ["git", "ls-tree", "-r", "--name-only", base, "--", "positions"], text=True
    ).splitlines()
    for path in paths:
        if path.endswith((".jsonl", ".manifest.json")):
            previous = subprocess.check_output(["git", "show", f"{base}:{path}"])
            current = pathlib.Path(path)
            if not current.is_file() or current.read_bytes() != previous:
                raise SystemExit(f"Published position set is immutable; add a new version: {path}")
print("Published position-set versions are unchanged.")
