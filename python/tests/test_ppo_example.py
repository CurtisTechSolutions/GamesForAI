from pathlib import Path
import runpy

import pytest

pytest.importorskip("sb3_contrib")


def test_real_ppo_updates_saves_loads_and_repeats_evaluation(tmp_path):
    example = runpy.run_path(str(Path(__file__).parents[2] / "examples/train_ppo.py"))
    args = example["parser"]().parse_args([
        "--game", "tictactoe", "--steps", "512", "--envs", "2", "--rollout", "32",
        "--batch-size", "32", "--epochs", "2", "--eval-games", "4",
        "--eval-opponent", "minimax:1", "--output", str(tmp_path),
    ])
    report = example["run"](args)
    assert report["parameters_changed"] and report["checkpoint_verified"]
    assert report["evaluation_reproduced"]
    assert report["actual_steps"] >= 512
    assert (tmp_path / "policy.zip").is_file()
    assert (tmp_path / "report.json").is_file()
    assert all(row["games"] == row["wins"] + row["draws"] + row["losses"] for row in report["after"])
