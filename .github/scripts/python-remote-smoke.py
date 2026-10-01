"""Compare the installed wheel's remote wrappers with a real local server."""
import json
from pathlib import Path
import re
import selectors
import subprocess
import tempfile

import numpy as np
from gamesforai import NativeEnv, connect, make

with tempfile.TemporaryDirectory() as directory:
    process = subprocess.Popen([
        str(Path("target/debug/gfa").resolve()), "serve", "--sqlite",
        str(Path(directory) / "remote.sqlite"), "--port", "0",
    ], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            if not selector.select(timeout=30):
                raise RuntimeError("server startup timed out")
            line = process.stdout.readline()
        match = re.search(r"http://127\.0\.0\.1:\d+", line)
        assert match, "server did not publish a local address"
        client = connect(match.group(), timeout=15)
        for game in ["tictactoe", "connect4", "chess"]:
            remote, local = client.native(game, seed=42), NativeEnv(game, seed=42)
            for _ in range(12):
                assert json.loads(remote.get_state()) == json.loads(local.get_state())
                if any(local.flags()):
                    break
                seat = local.current_players()[0]
                left, right = remote.frame(seat), local.frame(seat)
                np.testing.assert_array_equal(left["observation"], right["observation"])
                np.testing.assert_array_equal(left["action_mask"], right["action_mask"])
                action = int(np.flatnonzero(right["action_mask"])[0])
                assert remote.step(seat, action) == local.step(seat, action)
        for seat in [0, 1]:
            remote = client.make("tictactoe", seat=seat, opponent="minimax:3")
            local = make("tictactoe", seat=seat, opponent="minimax:3")
            left, _ = remote.reset(seed=19)
            right, _ = local.reset(seed=19)
            np.testing.assert_array_equal(left, right)
            while not any(remote.native.flags()):
                action = int(np.flatnonzero(local.action_masks())[0])
                a, b = remote.step(action), local.step(action)
                np.testing.assert_array_equal(a[0], b[0])
                assert a[1:4] == b[1:4]
        env = client.aec("tictactoe")
        env.reset(seed=0)
        checkpoint = env.get_state()
        env.step(0)
        env.set_state(checkpoint)
        for action in [0, 3, 1, 4, 2]:
            env.step(action)
        assert all(env.terminations.values())
        assert env.rewards == {"player_0": 1.0, "player_1": -1.0}
        assert isinstance(env.render(), str)
        env = client.make("connect4")
        obs, info = env.reset(seed=42, options={"position_set": "connect4-solved-positions@1"})
        assert info["curriculum"]["position_set"] == "connect4-solved-positions@1"
    finally:
        process.terminate()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise
        assert process.returncode == 0, "server shutdown failed"
print("Remote/native checkpoints, frames, Gym/AEC rewards and curricula verified")
