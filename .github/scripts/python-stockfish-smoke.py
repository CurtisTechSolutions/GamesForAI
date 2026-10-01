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
