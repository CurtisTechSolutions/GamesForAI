"""Validated, immutable position datasets for reproducible curricula."""
from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path

from ._native import NativeEnv, position_set_data


def _json(text):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result

    def invalid(value):
        raise ValueError(f"non-finite JSON value: {value}")

    return json.loads(text, object_pairs_hook=unique, parse_constant=invalid)


@dataclass(frozen=True)
class Position:
    id: str
    position: str
    config_json: str
    tags: tuple[str, ...]
    expected_json: str | None
    source: str


@dataclass(frozen=True)
class PositionSet:
    """A hash-verified set whose every position has passed engine validation.

    Expected puzzle answers are trusted dataset metadata, never policy input.
    Use load(name@version) for bundled sets or from_files(manifest, jsonl).
    """

    name: str
    version: int
    game: str
    sha256: str
    source: str
    license: str
    entries: tuple[Position, ...]
    _manifest: str
    _data: str

    @property
    def identifier(self):
        return f"{self.name}@{self.version}"

    @classmethod
    def load(cls, identifier):
        manifest, data = position_set_data(identifier)
        return cls.from_jsonl(manifest, data)

    @classmethod
    def from_files(cls, manifest, data):
        def read(path):
            with Path(path).open("rb") as file:
                value = file.read(8 * 1024 * 1024 + 1)
            if len(value) > 8 * 1024 * 1024:
                raise ValueError("position dataset exceeds 8 MiB")
            return value.decode("utf-8")
        return cls.from_jsonl(read(manifest), read(data))

    @classmethod
    def from_jsonl(cls, manifest_text, data):
        if not isinstance(data, str) or not isinstance(manifest_text, str):
            raise ValueError("manifest and data must be UTF-8 text")
        raw = data.encode("utf-8")
        if len(raw) > 8 * 1024 * 1024 or len(manifest_text.encode("utf-8")) > 64 * 1024:
            raise ValueError("position dataset size exceeds limit")
        manifest = _json(manifest_text)
        required = {"name", "version", "game", "sha256", "source", "license", "count"}
        if not isinstance(manifest, dict) or set(manifest) != required:
            raise ValueError("invalid position-set manifest fields")
        if not isinstance(manifest["name"], str) or not re.fullmatch(r"[a-z][a-z0-9-]{0,63}", manifest["name"]):
            raise ValueError("invalid position-set name")
        if type(manifest["version"]) is not int or manifest["version"] < 1:
            raise ValueError("version must be a positive integer")
        if type(manifest["count"]) is not int or not 1 <= manifest["count"] <= 4096:
            raise ValueError("count must be 1..4096")
        for field in ("game", "source", "license"):
            if not isinstance(manifest[field], str) or not manifest[field].strip():
                raise ValueError(f"manifest requires {field}")
        if manifest["sha256"] != hashlib.sha256(raw).hexdigest():
            raise ValueError("position-set content hash mismatch")
        lines = data.splitlines()
        if len(lines) != manifest["count"]:
            raise ValueError("position count does not match manifest")
        entries, ids = [], set()
        for line in lines:
            row = _json(line)
            if not isinstance(row, dict) or set(row) - {"id", "position", "config", "tags", "difficulty", "rating", "expected", "source"}:
                raise ValueError("invalid position entry fields")
            if not isinstance(row.get("id"), str) or not row["id"] or row["id"] in ids:
                raise ValueError("position ids must be unique nonempty strings")
            if not isinstance(row.get("position"), str) or not row["position"]:
                raise ValueError("entry requires position notation")
            tags = row.get("tags", [])
            if not isinstance(tags, list) or not all(isinstance(tag, str) for tag in tags):
                raise ValueError("tags must be a list of strings")
            source = row.get("source", manifest["source"])
            if not isinstance(source, str) or not source.strip():
                raise ValueError("entry source must be nonempty")
            config = row.get("config", {})
            if not isinstance(config, dict):
                raise ValueError("entry config must be an object")
            env = NativeEnv(manifest["game"], json.dumps(config, allow_nan=False))
            env.reset(0, row["position"])
            normalized = json.loads(env.get_state())["config"]
            entries.append(Position(
                row["id"], row["position"], json.dumps(normalized, sort_keys=True),
                tuple(tags), json.dumps(row["expected"], allow_nan=False) if "expected" in row else None, source
            ))
            ids.add(row["id"])
        return cls(
            manifest["name"], manifest["version"], manifest["game"], manifest["sha256"],
            manifest["source"], manifest["license"], tuple(entries), manifest_text, data
        )

    def validate_for(self, native):
        config = json.dumps(json.loads(native.get_state())["config"], sort_keys=True)
        if native.game_id != self.game or any(entry.config_json != config for entry in self.entries):
            raise ValueError("position set game/config does not match this environment")

    def sample(self, rng):
        return self.entries[int(rng.integers(len(self.entries)))]

    def checkpoint(self):
        return {"manifest": self._manifest, "data": self._data}
