import hashlib
import json
from dataclasses import FrozenInstanceError

import numpy as np
import pytest

from gamesforai import NativeEnv, PositionSet


def custom(rows, game="connect4", name="test-set"):
    data = "\n".join(json.dumps(row) for row in rows) + "\n"
    manifest = {
        "name": name, "version": 1, "game": game,
        "sha256": hashlib.sha256(data.encode()).hexdigest(),
        "source": "generated test", "license": "CC0-1.0", "count": len(rows),
    }
    return json.dumps(manifest), data


def test_bundled_sets_are_valid_and_reproducibly_sampled():
    for name, game, size in [("chess-endgames-basic@1", "chess", 6), ("connect4-solved-positions@1", "connect4", 7)]:
        dataset = PositionSet.load(name)
        assert dataset.identifier == name
        assert len(dataset.entries) == size
        assert dataset.license == "CC0-1.0"
        dataset.validate_for(NativeEnv(game))
        first, second = np.random.default_rng(41), np.random.default_rng(41)
        assert [dataset.sample(first).id for _ in range(100)] == [dataset.sample(second).id for _ in range(100)]
        for row in dataset.entries:
            env = NativeEnv(game, row.config_json)
            env.reset(0, row.position)
            assert env.current_players()
            if row.expected_json:
                expected = json.loads(row.expected_json)
                rewards, terminated, truncated = env.step_string(0, expected["winning_action"])
                assert terminated and not truncated and rewards[0] == expected["value"]
        with pytest.raises(FrozenInstanceError):
            dataset.entries[0].position = "changed"


def test_hash_counts_ids_and_all_positions_are_validated():
    row = {"id": "start", "position": '{"boards":[0,0],"to_move":0}'}
    manifest, data = custom([row])
    assert PositionSet.from_jsonl(manifest, data).entries[0].id == "start"
    with pytest.raises(ValueError, match="hash"):
        PositionSet.from_jsonl(manifest, data + " ")
    with pytest.raises(ValueError, match="unique"):
        PositionSet.from_jsonl(*custom([row, row]))
    with pytest.raises(ValueError):
        PositionSet.from_jsonl(*custom([row, {"id": "bad", "position": '{"boards":[0,0],"to_move":1}'}]))
    with pytest.raises(ValueError, match="match"):
        PositionSet.from_jsonl(manifest, data).validate_for(NativeEnv("chess"))
    changed = json.loads(manifest)
    changed["count"] = 2
    with pytest.raises(ValueError, match="count"):
        PositionSet.from_jsonl(json.dumps(changed), data)
    with pytest.raises(ValueError, match="duplicate"):
        PositionSet.from_jsonl(manifest[:-1] + ',"version":2}', data)
    with pytest.raises(ValueError):
        PositionSet.load("chess-endgames-basic")


def test_custom_dataset_files_and_checkpoint_roundtrip(tmp_path):
    manifest, data = custom([{"id": "start", "position": '{"boards":[0,0],"to_move":0}'}])
    meta, rows = tmp_path / "set.json", tmp_path / "set.jsonl"
    meta.write_text(manifest, encoding="utf-8")
    rows.write_text(data, encoding="utf-8")
    dataset = PositionSet.from_files(meta, rows)
    snapshot = dataset.checkpoint()
    restored = PositionSet.from_jsonl(snapshot["manifest"], snapshot["data"])
    assert restored == dataset
