import copy

import pytest

pa = pytest.importorskip("pyarrow")
pq = pytest.importorskip("pyarrow.parquet")

from gamesforai.parquet import trajectory_schema, write_parquet
from gamesforai.trajectories import EpisodeRecorder, select_records


def records():
    episode = EpisodeRecorder("tictactoe", ["one", "two"], seed=42)
    for action in [0, 3, 1, 4, 2]:
        episode.step(episode.current_players[0], action)
    return episode.records()


def test_typed_parquet_round_trip_with_streamed_row_groups(tmp_path):
    rows = records()
    path = tmp_path / "train.parquet"
    assert write_parquet(path, iter(rows), batch_size=2) == 5
    table = pq.read_table(path)
    assert table.to_pylist() == rows
    assert table.schema.equals(trajectory_schema(), check_metadata=True)
    assert table.schema.field("observation").type == pa.list_(pa.float32())
    assert table.schema.field("action_mask").type == pa.list_(pa.bool_())
    assert pq.ParquetFile(path).num_row_groups == 3


def test_filtered_and_empty_datasets_keep_schema(tmp_path):
    rows = records()
    path = tmp_path / "selected.parquet"
    assert write_parquet(path, select_records(rows, agent="two")) == 2
    assert all(row["outcome"] == "loss" for row in pq.read_table(path).to_pylist())
    assert write_parquet(path, []) == 0
    table = pq.read_table(path)
    assert table.num_rows == 0
    assert table.schema.equals(trajectory_schema(), check_metadata=True)


@pytest.mark.parametrize("field,value", [
    ("observation_shape", [999]),
    ("action_index", 999),
    ("reward", float("nan")),
    ("rewards", [1, 0]),
    ("format_version", True),
    ("episode_returns", [0]),
    ("terminated", 1),
    ("action_mask", [True]),
    ("seat", -1),
    ("outcome", "unknown"),
])
def test_bad_records_preserve_previous_output(tmp_path, field, value):
    path = tmp_path / "train.parquet"
    rows = records()
    write_parquet(path, rows)
    before = path.read_bytes()
    invalid = copy.deepcopy(rows[0])
    invalid[field] = value
    with pytest.raises(ValueError):
        write_parquet(path, [rows[0], invalid], batch_size=1)
    assert path.read_bytes() == before
    assert list(tmp_path.iterdir()) == [path]


def test_generator_failure_preserves_output(tmp_path):
    path = tmp_path / "train.parquet"
    write_parquet(path, [])
    before = path.read_bytes()

    def failed():
        yield records()[0]
        raise RuntimeError("source failure")

    with pytest.raises(RuntimeError):
        write_parquet(path, failed(), batch_size=1)
    assert path.read_bytes() == before
