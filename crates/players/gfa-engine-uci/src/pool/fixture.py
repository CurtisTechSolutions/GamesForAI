"""Test-only deterministic engine. Production has no unsandboxed launch path."""
import os
import sys
import time

mode = sys.argv[1]
if mode == "blocked_input":
    time.sleep(30)
    sys.exit(0)

for command in sys.stdin:
    command = command.strip()
    if command == "uci":
        print(f"id name Fixture-{os.getpid()}", flush=True)
        print("option name Threads type spin default 1 min 1 max 4", flush=True)
        print("option name Hash type spin default 32 min 1 max 256", flush=True)
        print("option name MultiPV type spin default 1 min 1 max 16", flush=True)
        print("option name Skill Level type spin default 20 min 0 max 20", flush=True)
        print("uciok", flush=True)
    elif command == "isready":
        print("readyok", flush=True)
    elif command.startswith("go "):
        if mode == "crash":
            sys.exit(3)
        if mode == "hang":
            time.sleep(30)
        elif mode == "oversized":
            print("x" * 8193, flush=True)
        elif mode == "invalid_utf8":
            sys.stdout.buffer.write(b"\xff\n")
            sys.stdout.buffer.flush()
        elif mode == "none":
            print("bestmove (none)", flush=True)
        else:
            print("info depth 3 nodes 42 multipv 1 score cp 23 pv e2e4 e7e5", flush=True)
            print("info nodes 43 time 1", flush=True)
            print("bestmove e2e4 ponder e7e5", flush=True)
