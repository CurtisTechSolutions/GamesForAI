"""Typed, streaming Parquet export; install gamesforai[datasets] to use."""
from __future__ import annotations

import math
import os
import tempfile
from pathlib import Path

from .trajectories import _time


def trajectory_schema():
    """Stable v1 columns shared with EpisodeRecorder JSONL records."""
    try:
        import pyarrow as pa
    except ImportError as error:
        raise ImportError("Parquet export requires gamesforai[datasets]") from error
    fields = []
    types = {
        "format_version": pa.int32(), "episode_id": pa.string(), "game": pa.string(),
        "engine_version": pa.string(), "config_json": pa.string(), "seed": pa.string(),
        "created_at": pa.string(), "ply": pa.int32(), "seat": pa.int32(), "agent": pa.string(),
        "agent_rating": pa.float64(), "observation": pa.list_(pa.field("element", pa.float32())),
        "observation_shape": pa.list_(pa.field("element", pa.int32())), "action_mask": pa.list_(pa.field("element", pa.bool_())),
        "observation_text": pa.string(), "action": pa.string(), "action_index": pa.int32(),
        "reasoning": pa.string(), "transcript_json": pa.string(), "rewards": pa.list_(pa.field("element", pa.float64())),
        "reward": pa.float64(), "next_observation": pa.list_(pa.field("element", pa.float32())),
        "next_action_mask": pa.list_(pa.field("element", pa.bool_())), "next_to_act": pa.list_(pa.field("element", pa.int32())),
        "terminated": pa.bool_(), "truncated": pa.bool_(),
        "outcome": pa.string(), "episode_returns": pa.list_(pa.field("element", pa.float64())),
    }
    for name, dtype in types.items():
        fields.append(pa.field(name, dtype, nullable=name in {"agent_rating", "reasoning", "transcript_json"}))
    return pa.schema(fields, metadata={
        b"gamesforai.format": b"trajectory", b"gamesforai.version": b"1",
        b"gamesforai.transition": b"one game turn; next observation belongs to acting seat",
    })


def _validate(row, names):
    if not isinstance(row, dict) or set(row) != names or type(row["format_version"]) is not int or row["format_version"] != 1:
        raise ValueError("expected a complete version 1 trajectory record")
    if row["outcome"] not in {"win", "loss", "draw", "truncated"}:
        raise ValueError("unknown trajectory outcome")
    for field in ("terminated", "truncated"):
        if type(row[field]) is not bool:
            raise ValueError("trajectory flags must be boolean")
    if row["terminated"] and row["truncated"]:
        raise ValueError("a turn cannot terminate and truncate simultaneously")
    if (row["truncated"] and row["outcome"] != "truncated") or (
        row["terminated"] and row["outcome"] == "truncated"
    ):
        raise ValueError("terminal trajectory outcome does not match its flags")
    shape = row["observation_shape"]
    if not isinstance(shape, list) or not shape or len(shape) > 8 or any(type(v) is not int or v < 1 for v in shape):
        raise ValueError("invalid observation shape")
    length = math.prod(shape)
    for field in ("observation", "next_observation", "rewards", "episode_returns"):
        values = row[field]
        if not isinstance(values, list) or any(
            type(v) not in (int, float) or not math.isfinite(v) for v in values
        ):
            raise ValueError("trajectory numeric arrays must contain finite numbers")
        if field in ("observation", "next_observation"):
            if len(values) != length:
                raise ValueError("observation length does not match shape")
            if any(abs(value) > 3.4028234663852886e38 for value in values):
                raise ValueError("observation exceeds finite float32 range")
    for field in ("action_mask", "next_action_mask"):
        if not isinstance(row[field], list) or any(type(value) is not bool for value in row[field]):
            raise ValueError("action masks must be boolean lists")
    mask, action = row["action_mask"], row["action_index"]
    if len(row["next_action_mask"]) != len(mask) or type(action) is not int or not 0 <= action < len(mask) or not mask[action]:
        raise ValueError("trajectory action must be allowed by its mask")
    seats, seat = len(row["rewards"]), row["seat"]
    if type(seat) is not int or not 0 <= seat < seats or len(row["episode_returns"]) != seats:
        raise ValueError("trajectory seat does not match reward arrays")
    if not isinstance(row["next_to_act"], list) or any(type(value) is not int or not 0 <= value < seats for value in row["next_to_act"]):
        raise ValueError("invalid next acting seats")
    if type(row["reward"]) not in (int, float) or not math.isfinite(row["reward"]) or row["reward"] != row["rewards"][seat]:
        raise ValueError("acting-seat reward differs from reward vector")
    if type(row["ply"]) is not int or row["ply"] < 0:
        raise ValueError("ply must be nonnegative")
    rating = row["agent_rating"]
    if rating is not None and (type(rating) not in (int, float) or not math.isfinite(rating)):
        raise ValueError("agent rating must be finite")
    for field in ("episode_id", "game", "engine_version", "config_json", "seed", "created_at", "agent", "observation_text", "action"):
        if not isinstance(row[field], str) or not row[field]:
            raise ValueError("trajectory text fields must be nonempty strings")
    for field in ("reasoning", "transcript_json"):
        if row[field] is not None and not isinstance(row[field], str):
            raise ValueError("optional trajectory text must be a string")
    _time(row["created_at"])


def write_parquet(path, records, *, batch_size=256):
    """Stream bounded row groups into an atomically replaced Parquet file."""
    schema = trajectory_schema()
    import pyarrow as pa
    import pyarrow.parquet as pq

    if type(batch_size) is not int or not 1 <= batch_size <= 8192:
        raise ValueError("batch_size must be 1..8192")
    path, temporary = Path(path), None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as file:
            temporary = Path(file.name)
        count, batch = 0, []
        with pq.ParquetWriter(temporary, schema, compression="zstd") as writer:
            for row in records:
                _validate(row, set(schema.names))
                batch.append(row)
                count += 1
                if len(batch) == batch_size:
                    writer.write_table(pa.Table.from_pylist(batch, schema=schema))
                    batch.clear()
            if batch:
                writer.write_table(pa.Table.from_pylist(batch, schema=schema))
        with temporary.open("r+b") as file:
            os.fsync(file.fileno())
        os.replace(temporary, path)
        return count
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
