"""Exercise the actual MCP CLI, clean stdout, game loop, EOF, signal, and restart."""
import json
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time

binary = Path("target/debug/gfa").resolve()


class Client:
    def __init__(self, database, *options):
        self.errors = tempfile.TemporaryFile(mode="w+t")
        self.process = subprocess.Popen(
            [str(binary), "mcp", "--sqlite", str(database), *options],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.errors,
            text=True, bufsize=1,
        )
        self.lines = queue.Queue()
        self.sequence = 0
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        for line in self.process.stdout:
            self.lines.put(line)
        self.lines.put(None)

    def send(self, message):
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def request(self, method, params=None):
        self.sequence += 1
        request_id = self.sequence
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params or {}})
        while True:
            line = self.lines.get(timeout=30)
            if line is None:
                raise RuntimeError("MCP stdout closed before a response")
            message = json.loads(line)  # Startup/log banners contaminating stdout fail here.
            if message.get("id") == request_id:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message["result"]

    def initialize(self):
        result = self.request("initialize", {
            "protocolVersion": "2025-11-25", "capabilities": {},
            "clientInfo": {"name": "gfa-smoke", "version": "1"},
        })
        assert result["protocolVersion"] == "2025-11-25"
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        return result

    def tool(self, name, arguments, failed=False):
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        assert result.get("isError", False) == failed, result
        assert result["content"] and result["content"][0]["type"] == "text"
        return result["structuredContent"]["error" if failed else "data"]

    def close(self, signal=False):
        if self.process.poll() is None:
            if signal:
                self.process.terminate()  # Keep stdin open to exercise blocking-reader cleanup.
            else:
                self.process.stdin.close()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
            raise
        if not self.process.stdin.closed:
            self.process.stdin.close()
        self.reader.join(timeout=2)
        self.errors.seek(0)
        diagnostics = self.errors.read()
        self.errors.close()
        assert self.process.returncode == 0, diagnostics
        assert not self.reader.is_alive()


with tempfile.TemporaryDirectory() as directory:
    database = Path(directory) / "mcp.sqlite"
    client = Client(database)
    try:
        client.initialize()
        names = {tool["name"] for tool in client.request("tools/list")["tools"]}
        assert {"list_games", "create_match", "make_move", "get_state"} <= names
        created = client.tool("create_match", {"game_id": "tictactoe", "opponent": "random", "seed": 23})
        match_id = created["state"]["match_id"]
        action = created["state"]["legal_actions"][0]
        error = client.tool("make_move", {"match_id": match_id, "action": "invalid"}, failed=True)
        assert error["details"]["legal_actions"]
        moved = client.tool("make_move", {"match_id": match_id, "action": action, "reasoning": "smoke"})
        assert moved["state"]["turn"] == 2 and len(moved["opponent_actions"]) == 1
    finally:
        client.close()
    restarted = Client(database)
    try:
        restarted.initialize()
        assert restarted.tool("get_state", {"match_id": match_id}) == moved["state"]
    finally:
        restarted.close(signal=True)
    spectator = Client(database, "--spectator")
    try:
        spectator.initialize()
        view = spectator.tool("get_state", {"match_id": match_id})
        assert view["you"] is None and view["legal_actions"] == []
        assert spectator.tool("resign", {"match_id": match_id}, failed=True)["code"] == "FORBIDDEN"
    finally:
        spectator.close()
    uninitialized = Client(database)
    # Allow signal handler installation, then leave stdin open during shutdown.
    time.sleep(1)
    uninitialized.close(signal=True)

print("MCP stdio game loop, clean stdout, persistence, EOF, and signal shutdown passed.")
