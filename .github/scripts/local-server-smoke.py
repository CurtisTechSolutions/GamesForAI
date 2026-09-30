"""Exercise the actual CLI, HTTP listener, graceful shutdown, and restart."""
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import tempfile
import urllib.request

binary = Path("target/debug/gfa").resolve()


def start(database):
    variable = os.environ.get("GFA_SMOKE_POSTGRES_ENV")
    storage = ["--postgres-env", variable] if variable else ["--sqlite", str(database)]
    process = subprocess.Popen(
        [str(binary), "serve", *storage, "--port", "0"],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            if not selector.select(timeout=15):
                raise RuntimeError("Server startup timed out")
            line = process.stdout.readline()
        match = re.search(r"http://127\.0\.0\.1:\d+", line)
        if not match:
            raise RuntimeError(f"Server failed to start: {line}")
        return process, match.group()
    except BaseException:
        stop(process)
        raise


def stop(process):
    if process.poll() is None:
        process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
        raise
    if process.returncode != 0:
        raise RuntimeError(f"Server shutdown failed: {process.returncode}")


def request(base, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(base + path, data=data, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=5) as response:
        return json.load(response)


with tempfile.TemporaryDirectory() as directory:
    database = Path(directory) / "matches.sqlite"
    process, base = start(database)
    try:
        assert request(base, "/healthz")["status"] == "ok"
        initial = request(base, "/v1/matches", {"game_id": "tictactoe", "seed": 42})
        match_id = initial["match_id"]
        moved = request(base, f"/v1/matches/{match_id}/actions", {"seat": 0, "turn": 0, "action": "r2c2"})
        assert moved["state"]["turn"] == 1
    finally:
        stop(process)
    process, base = start(database)
    try:
        replay = request(base, f"/v1/matches/{match_id}/replay?seat=0")
        assert len(replay["states"]) == 2
        assert replay["states"][1] == moved["state"]
    finally:
        stop(process)
print("Local CLI/HTTP/database restart smoke test passed.")
