import json
from pathlib import Path
import subprocess
import sys
import types

import numpy as np
import pytest

from gamesforai.tournament_cli import _write_report, main
from gamesforai.tournament_config import ConfigurationError, load_agent, load_document, opponent_names


def agent_file(tmp_path, name="candidate", **fields):
    path = tmp_path / f"{name}.yaml"
    path.write_text(json.dumps({"id": name, "type": "builtin", "opponent": "random", **fields}))
    return path


def arguments(tmp_path, agent, *extra):
    return ["--game", "tictactoe", "--agents", str(agent),
            "--opponents", "minimax:1..2", "--games", "2",
            "--report", str(tmp_path / "report.json"), *extra]


def test_cli_selected_ladder_report_and_jsonl(tmp_path, capsys):
    path = agent_file(tmp_path)
    output = tmp_path / "episodes"
    assert main(arguments(tmp_path, path, "--trajectories", str(output))) == 0
    report = json.loads((tmp_path / "report.json").read_text())
    assert len(report["matches"]) == len(report["artifacts"]) == 4
    assert report["matchups"] == [["candidate", "builtin:minimax:1"], ["candidate", "builtin:minimax:2"]]
    assert all(match["status"] == "completed" for match in report["matches"])
    assert len(list(output.glob("*.jsonl"))) == 4
    for artifact in report["artifacts"]:
        rows = [json.loads(line) for line in Path(artifact["path"]).read_text().splitlines()]
        assert rows[-1]["terminated"]
        assert rows[0]["episode_id"] == artifact["episode_id"]
    assert json.loads(capsys.readouterr().out)["results"] == {"completed": 4}
    # Existing training data is never replaced by a second run.
    before = (tmp_path / "report.json").read_bytes()
    assert main(arguments(tmp_path, path, "--trajectories", str(output))) == 1
    assert (tmp_path / "report.json").read_bytes() == before


def test_cli_python_factory_receives_seed_seat_and_parameters(tmp_path, monkeypatch):
    calls = []
    module = types.ModuleType("test_model_factory")

    def factory(*, seed, seat, checkpoint):
        calls.append((seed, seat, checkpoint))
        return lambda obs, info: int(np.flatnonzero(info["action_mask"])[0])

    module.make_policy = factory
    monkeypatch.setitem(sys.modules, module.__name__, module)
    path = tmp_path / "python.yaml"
    path.write_text("id: checkpoint-123\ntype: python\nfactory: test_model_factory:make_policy\n"
                    "params:\n  checkpoint: weights/model.pt\n")
    assert main(arguments(tmp_path, path, "--opponents", "random", "--seed", "7")) == 0
    assert calls == [(7, 0, "weights/model.pt"), (8, 1, "weights/model.pt")]


def test_cli_parquet_and_module_entry_point(tmp_path):
    import pyarrow.parquet as pq
    path = agent_file(tmp_path)
    output = tmp_path / "parquet"
    result = subprocess.run([sys.executable, "-m", "gamesforai.tournament_cli",
                             *arguments(tmp_path, path, "--opponents", "random",
                                        "--trajectories", str(output), "--format", "parquet")],
                            capture_output=True, text=True, timeout=60)
    assert result.returncode == 0, result.stderr
    files = sorted(output.glob("*.parquet"))
    assert len(files) == 2
    assert all(pq.read_table(file).num_rows > 0 for file in files)


def test_failed_model_has_nonzero_exit_and_redacted_report(tmp_path, monkeypatch, capsys):
    module = types.ModuleType("private_policy")

    def factory(**kwargs):
        raise RuntimeError("super-secret-api-key")

    module.factory = factory
    monkeypatch.setitem(sys.modules, module.__name__, module)
    path = tmp_path / "bad.json"
    path.write_text(json.dumps({"id": "failed-model", "type": "python", "factory": "private_policy:factory"}))
    assert main(arguments(tmp_path, path, "--opponents", "random")) == 1
    text = (tmp_path / "report.json").read_text()
    assert "super-secret" not in text + capsys.readouterr().out
    report = json.loads(text)
    assert all(row["rated_games"] == 0 for row in report["standings"])
    assert all(match["failure"]["code"] == "policy_factory" for match in report["matches"])


@pytest.mark.parametrize("value", [
    "id: first\nid: second\n", "id: &ref [*ref]\n",
    "id: !!python/object/apply:os.system ['never-execute']\n",
    "id: .nan\n", "id: 2026-01-01\n", "1: value\n",
    "id: " + "[" * 33 + "0" + "]" * 33,
    "id: " + "x" * 65536,
], ids=["duplicate", "recursive-alias", "object-tag", "nan", "timestamp", "nonstring-key", "depth", "size"])
def test_config_rejects_ambiguous_or_unsafe_documents(tmp_path, value):
    path = tmp_path / "input.yaml"
    path.write_text(value)
    with pytest.raises(ConfigurationError):
        load_document(path)


def test_chat_configuration_is_validated_without_inference_or_secret_serialization():
    agent = load_agent({"id": "http-v1", "type": "chat", "base_url": "http://localhost:8000/v1",
                        "model": "my-model", "api_key_env": "MY_MODEL_TOKEN"}, lambda: None)
    policy = agent.policy_factory(1, 0)
    assert policy.model == "my-model" and policy.api_key_env == "MY_MODEL_TOKEN"
    for document in [
        {"id": "http", "type": "chat", "api_key": "super-secret"},
        {"id": "python", "type": "python", "factory": "private:not_here", "params": {"seed": 0}},
        {"id": "bad", "type": []},
    ]:
        with pytest.raises(ConfigurationError):
            load_agent(document, lambda: None)


def test_invalid_config_error_does_not_echo_values(tmp_path, capsys):
    path = tmp_path / "private.yaml"
    path.write_text("id: secret\nbase_url: [super-secret-api-key\n")
    assert main(arguments(tmp_path, path)) == 2
    captured = capsys.readouterr()
    assert "super-secret" not in captured.err
    assert not (tmp_path / "report.json").exists()


def test_atomic_report_preserves_previous_file_on_write_failure(tmp_path, monkeypatch):
    path = tmp_path / "report.json"
    path.write_text("previous")

    def fail(*args):
        raise OSError("disk full")

    monkeypatch.setattr("gamesforai.tournament_cli.os.replace", fail)
    with pytest.raises(OSError):
        _write_report(path, {"new": True})
    assert path.read_text() == "previous"
    assert not list(tmp_path.glob(".gfa-report-*"))


def test_ladder_range_and_selection_validation():
    assert opponent_names("random,stockfish:1..3,minimax:10") == [
        "random", "stockfish:1", "stockfish:2", "stockfish:3", "minimax:10"]
    for value in ["stockfish:0", "mcts:11", "minimax:5..1", "random,random", ""]:
        with pytest.raises(ConfigurationError):
            opponent_names(value)
