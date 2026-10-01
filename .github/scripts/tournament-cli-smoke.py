"""Exercise the Rust CLI forwarding into a real SDK/Stockfish tournament."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

with tempfile.TemporaryDirectory(prefix="gfa tournament ") as directory:
    root = Path(directory)
    agent = root / "random agent.yaml"
    agent.write_text("id: random-snapshot\ntype: builtin\nopponent: random\n")
    config = root / "short game.json"
    config.write_text(json.dumps({"max_plies": 4}))
    report = root / "engine report.json"
    env = {**os.environ, "GFA_PYTHON": sys.executable}
    command = ["cargo", "run", "-p", "gfa-cli", "--locked", "--", "tournament",
               "--game", "chess", "--agents", str(agent), "--opponents", "stockfish:1..2",
               "--games", "2", "--config", str(config), "--stockfish", "/usr/games/stockfish",
               "--report", str(report), "--trajectories", str(root / "episodes")]
    subprocess.run(command, env=env, check=True, timeout=180)
    data = json.loads(report.read_text())
    assert len(data["matches"]) == len(data["artifacts"]) == 4
    assert all(match["status"] == "truncated" for match in data["matches"])
    assert all(row["rated_games"] == 0 for row in data["standings"])
    assert all(Path(row["path"]).is_file() for row in data["artifacts"])
    for spec in data["agent_specs"][1:]:
        assert len(spec["stockfish"]["engine_sha256"]) == 64
    # The shim must also preserve the Python command's nonzero exit status.
    failed = subprocess.run(["cargo", "run", "-p", "gfa-cli", "--locked", "--",
                             "tournament", "--game", "chess"], env=env, timeout=60)
    assert failed.returncode == 2
print("Rust CLI / Python SDK / Stockfish tournament verified")
