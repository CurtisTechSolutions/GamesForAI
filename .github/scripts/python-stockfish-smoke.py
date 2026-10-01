"""Real Python/native engine integration, executed in the Linux sandbox job."""
from gamesforai import NativeEnv
from gamesforai._native import NativeUciPool

pool = NativeUciPool("/usr/games/stockfish", workers=1)
env = NativeEnv("chess", '{"max_plies":8}', seed=42)
for ply in range(8):
    seat = env.current_players()[0]
    before = env.get_state()
    action = pool.choose(env, seat, level=3, seed=42 + ply, time_ms=5000)
    assert env.get_state() == before
    assert env.frame(seat)["action_mask"][action]
    env.step(seat, action)
assert env.flags() == (False, True)
print("Python Stockfish: 8 legal plies, unchanged planning state, bounded truncation")

import numpy as np
from gamesforai import StockfishPool, make

shared = StockfishPool("/usr/games/stockfish", workers=1)
for learner in (0, 1):
    gym = make("chess", config={"max_plies": 4}, seat=learner,
               opponent="stockfish:5", stockfish_pool=shared)
    obs, info = gym.reset(seed=42)
    for _ in range(2):
        obs, reward, terminated, truncated, info = gym.step(int(np.flatnonzero(info["action_mask"])[0]))
        if terminated or truncated:
            break
    assert truncated and not terminated
    assert gym.get_state()["opponent"]["engine_sha256"] == shared.identity()["engine_sha256"]
print("Gym Stockfish: both learner seats complete bounded episodes")
