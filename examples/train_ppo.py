"""Train and evaluate a masked policy using an installed gamesforai wheel."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from importlib.metadata import version
from pathlib import Path

import numpy as np
import torch
from sb3_contrib import MaskablePPO
from stable_baselines3.common.vec_env import DummyVecEnv

from gamesforai import NativeEnv, make


def digest(policy):
    result = hashlib.sha256()
    for name, tensor in sorted(policy.state_dict().items()):
        result.update(name.encode())
        result.update(tensor.detach().cpu().contiguous().numpy().tobytes())
    return result.hexdigest()


def opening(game, seed, plies):
    env = NativeEnv(game, seed=seed)
    rng = np.random.default_rng(seed)
    for _ in range(plies):
        actors = env.current_players()
        if not actors:
            raise ValueError("opening unexpectedly ended the game")
        env.step(actors[0], int(rng.choice(np.flatnonzero(env.action_mask(actors[0])))))
    return json.dumps(json.loads(env.get_state())["state"])


def evaluate(model, game, opponent, episodes, seed, opening_plies):
    results = []
    for episode in range(episodes):
        env = make(game, seat=episode % 2, opponent=opponent)
        obs, info = env.reset(seed=seed + episode, options={
            "position": opening(game, seed + episode, opening_plies)
        })
        total = 0.0
        for _ in range(42):
            action, _ = model.predict(obs, action_masks=env.action_masks(), deterministic=True)
            obs, reward, terminated, truncated, info = env.step(int(action))
            total += reward
            if terminated or truncated:
                break
        else:
            raise RuntimeError("evaluation episode exceeded game bounds")
        env.close()
        results.append(total)
    wins = sum(reward > 0 for reward in results)
    draws = sum(reward == 0 for reward in results)
    probability, z = wins / episodes, 1.959963984540054
    denominator = 1 + z * z / episodes
    center = (probability + z * z / (2 * episodes)) / denominator
    radius = z * math.sqrt(probability * (1 - probability) / episodes + z * z / (4 * episodes**2)) / denominator
    return {
        "opponent": opponent, "games": episodes, "wins": wins, "draws": draws,
        "losses": episodes - wins - draws, "win_rate": probability,
        "win_rate_wilson95": [max(0.0, center - radius), min(1.0, center + radius)],
        "score": (wins + 0.5 * draws) / episodes, "mean_return": float(np.mean(results)),
        "evaluation_seed": seed, "random_opening_plies": opening_plies,
    }


def run(args):
    if args.game not in ("tictactoe", "connect4"):
        raise ValueError("example supports Tic-Tac-Toe and Connect Four")
    if not 1 <= args.envs <= 64 or not 1 <= args.threads <= 64:
        raise ValueError("envs and threads must be 1..64")
    if args.steps < 1 or args.eval_games < 2 or not 0 <= args.opening_plies <= 2:
        raise ValueError("require positive steps, >=2 evaluation games, and 0..2 opening plies")
    if args.rollout < 2 or args.epochs < 1 or args.batch_size < 2:
        raise ValueError("invalid PPO rollout, batch, or epoch sizes")
    if args.batch_size > args.rollout * args.envs:
        raise ValueError("batch size exceeds rollout size")
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    torch.set_num_threads(args.threads)
    torch.use_deterministic_algorithms(True)
    environments = DummyVecEnv([
        (lambda seat=i % 2: make(args.game, seat=seat, opponent=args.opponent))
        for i in range(args.envs)
    ])
    environments.seed(args.seed)
    model = MaskablePPO(
        "MlpPolicy", environments, n_steps=args.rollout, batch_size=args.batch_size,
        n_epochs=args.epochs, learning_rate=3e-4, gamma=0.99, ent_coef=0.01,
        policy_kwargs={"net_arch": [64, 64]}, seed=args.seed, device="cpu", verbose=1,
    )
    evaluation_seed = args.seed + 1_000_000
    opponents = list(dict.fromkeys(["random", args.eval_opponent]))
    initial_digest = digest(model.policy)
    before = [
        evaluate(model, args.game, opponent, args.eval_games, evaluation_seed, args.opening_plies)
        for opponent in opponents
    ]
    model.learn(total_timesteps=args.steps)
    trained_digest = digest(model.policy)
    if trained_digest == initial_digest:
        raise RuntimeError("training did not change policy parameters")
    model_path = output / "policy.zip"
    model.save(model_path)
    restored = MaskablePPO.load(model_path, device="cpu")
    if digest(restored.policy) != trained_digest:
        raise RuntimeError("saved policy parameters differ after loading")
    after = [
        evaluate(restored, args.game, opponent, args.eval_games, evaluation_seed, args.opening_plies)
        for opponent in opponents
    ]
    repeat = evaluate(restored, args.game, opponents[-1], args.eval_games, evaluation_seed, args.opening_plies)
    if repeat != after[-1]:
        raise RuntimeError("fixed evaluation did not reproduce")
    report = {
        "game": args.game, "training_seed": args.seed, "requested_steps": args.steps,
        "actual_steps": model.num_timesteps, "training_opponent": args.opponent,
        "parameters_changed": True, "checkpoint_verified": True,
        "evaluation_reproduced": True, "policy_sha256": trained_digest,
        "initial_policy_sha256": initial_digest, "before": before, "after": after,
        "versions": {name: version(name) for name in ("gamesforai", "sb3-contrib", "stable-baselines3", "torch", "numpy")},
        "configuration": vars(args),
    }
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    environments.close()
    print(json.dumps(report, indent=2))
    return report


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--game", choices=["tictactoe", "connect4"], default="connect4")
    result.add_argument("--steps", type=int, default=250_000)
    result.add_argument("--envs", type=int, default=8)
    result.add_argument("--rollout", type=int, default=128)
    result.add_argument("--batch-size", type=int, default=64)
    result.add_argument("--epochs", type=int, default=4)
    result.add_argument("--threads", type=int, default=1)
    result.add_argument("--seed", type=int, default=42)
    result.add_argument("--opponent", default="random")
    result.add_argument("--eval-opponent", default="minimax:3")
    result.add_argument("--eval-games", type=int, default=100)
    result.add_argument("--opening-plies", type=int, default=2)
    result.add_argument("--output", default="training-output")
    return result


if __name__ == "__main__":
    run(parser().parse_args())
