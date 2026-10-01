"""Evaluate caller-owned frozen policies from the command line."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys
import tempfile

from .stockfish import StockfishPool
from .tournament import Agent, run_round_robin
from .tournament_config import ConfigurationError, load_agent, load_document, opponent_names
from .trajectories import write_jsonl


def _write_report(path, report):
    path = Path(path)
    encoded = json.dumps(report, indent=2, allow_nan=False) + "\n"
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                         prefix=".gfa-report-", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--game", required=True)
    result.add_argument("--agents", required=True, help="comma-separated YAML/JSON agent files")
    result.add_argument("--opponents", help="random,minimax:1..3,mcts:5,stockfish:1..8")
    result.add_argument("--games", type=int, default=100, help="even games per selected pair (2..1000)")
    result.add_argument("--seed", type=int, default=42)
    result.add_argument("--config", help="game configuration JSON/YAML file")
    result.add_argument("--position-set", help="bundled immutable name@version")
    result.add_argument("--report", type=Path, default=Path("tournament-report.json"))
    result.add_argument("--trajectories", type=Path, help="new directory for completed episode files")
    result.add_argument("--format", choices=["jsonl", "parquet"], default="jsonl")
    result.add_argument("--stockfish", help="absolute engine path; otherwise GFA_STOCKFISH_PATH")
    result.add_argument("--stockfish-workers", type=int, default=1)
    result.add_argument("--stockfish-time-ms", type=int, default=5000)
    return result


def execute(args):
    paths = [path.strip() for path in args.agents.split(",")]
    if not all(paths) or len(paths) > 32:
        raise ConfigurationError("--agents requires 1..32 nonempty file paths")
    if args.format != "jsonl" and args.trajectories is None:
        raise ConfigurationError("--format requires --trajectories")
    if not args.report.parent.is_dir():
        raise ConfigurationError("report parent directory must already exist")
    config = {} if args.config is None else load_document(args.config)
    documents = [load_document(path) for path in paths]
    shared_pool = None

    def pool():
        nonlocal shared_pool
        if shared_pool is None:
            shared_pool = StockfishPool(args.stockfish, workers=args.stockfish_workers,
                                       time_ms=args.stockfish_time_ms)
        return shared_pool

    agents = [load_agent(document, pool) for document in documents]
    matchups = None
    if args.opponents is not None:
        baselines = [
            Agent(f"builtin:{name}", opponent=name,
                  stockfish_pool=pool() if name.startswith("stockfish:") else None)
            for name in opponent_names(args.opponents)
        ]
        matchups = [(agent.id, baseline.id) for agent in agents for baseline in baselines]
        agents.extend(baselines)
    artifacts = []
    writer = write_jsonl
    if args.format == "parquet":
        from .parquet import write_parquet
        # Resolve the optional dependency before playing any games.
        import pyarrow  # noqa: F401
        writer = write_parquet
    output = args.trajectories
    if output is not None:
        # Never mix a partial/new run with an existing run's training data.
        output.mkdir(parents=True, exist_ok=False)

    def sink(summary, rows):
        path = output / f"episode-{len(artifacts):06d}.{args.format}"
        writer(path, rows)
        artifacts.append({"episode_id": summary["id"], "path": str(path.resolve()),
                          "trajectory_sha256": summary["trajectory_sha256"]})

    report = run_round_robin(
        args.game, agents, games_per_pair=args.games, seed=args.seed, config=config,
        position_set=args.position_set, matchups=matchups,
        on_episode=None if output is None else sink,
    )
    report["artifacts"] = artifacts
    _write_report(args.report, report)
    return report


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        report = execute(args)
    except ConfigurationError as error:
        print(f"Configuration error: {error}", file=sys.stderr)
        return 2
    except Exception as error:
        # Model/configuration exceptions can contain credentials or response text.
        print(f"Tournament aborted ({type(error).__name__}); no new complete report was written. "
              "Check the game, schedule, engine and output paths.", file=sys.stderr)
        return 1
    counts = {}
    for match in report["matches"]:
        counts[match["status"]] = counts.get(match["status"], 0) + 1
    print(json.dumps({"report": str(args.report.resolve()), "results": counts}, sort_keys=True))
    return 1 if counts.get("failed", 0) or counts.get("invalid_start", 0) else 0


if __name__ == "__main__":
    raise SystemExit(main())
