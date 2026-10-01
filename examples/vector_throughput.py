"""Measure native batched turns plus NumPy transfer, excluding model inference."""
import argparse
import json
import os
import platform
import statistics
import time
from pathlib import Path

import numpy as np

from gamesforai import VectorEnv


def benchmark(n, workers, batches, samples, seed):
    env = VectorEnv("connect4", n, threads=workers, seed=seed)
    rng = np.random.default_rng(seed)
    frame = env.frame()
    rates, total_resets = [], 0
    for sample in range(samples + 1):
        elapsed = 0.0
        for _ in range(batches):
            actions = [int(rng.choice(np.flatnonzero(mask))) for mask in frame["action_mask"]]
            started = time.perf_counter_ns()
            frame = env.step(actions)
            elapsed += (time.perf_counter_ns() - started) / 1e9
            indices = np.flatnonzero(frame["terminated"] | frame["truncated"]).tolist()
            if indices:
                seeds = [int(rng.integers(0, 2**63)) for _ in indices]
                frame = env.reset_at(indices, seeds)
                total_resets += len(indices)
        if sample:  # Warm up allocations, workers, and caches before recording.
            rates.append(n * batches / elapsed)
    return {
        "game": "connect4", "batch_size": n, "requested_workers": workers,
        "samples": samples, "turns_per_sample": n * batches,
        "median_steps_per_second": statistics.median(rates),
        "min_steps_per_second": min(rates), "max_steps_per_second": max(rates),
        "episode_resets_including_warmup": total_resets, "seed": seed,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--n", type=int, default=256)
    parser.add_argument("--batches", type=int, default=64)
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--output", default="vector-benchmark.json")
    args = parser.parse_args()
    if not 1 <= args.n <= 4096 or not 1 <= args.batches <= 10000 or not 1 <= args.samples <= 100:
        parser.error("n, batches, or samples outside benchmark limits")
    workers = sorted({1, min(8, os.cpu_count() or 1)})
    report = {
        "format_version": 1, "python": platform.python_version(),
        "platform": platform.platform(), "logical_cpus": os.cpu_count(),
        "measurement": "VectorEnv.step including Python call, Rust transition, projections, and owned NumPy arrays",
        "excluded": ["action sampling/model inference", "explicit reset_at calls", "one warmup sample"],
        "games": [benchmark(args.n, count, args.batches, args.samples, args.seed) for count in workers],
        "prd_reference": {"steps_per_second": 1_000_000, "cores": 8},
        "reference_hardware_verified": False,
    }
    Path(args.output).write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
