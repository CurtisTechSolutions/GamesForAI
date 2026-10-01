import copy
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import threading
from types import SimpleNamespace
from urllib.parse import unquote, urlsplit

import numpy as np
import pytest

from gamesforai import NativeEnv, connect, games, make
from gamesforai.remote import RemoteError


@pytest.fixture
def server():
    state = SimpleNamespace(requests=[], fail_observe=False, malformed=False, response=None)

    def operation(row):
        if row["op"] == "create":
            env = NativeEnv(row["game_id"], json.dumps(row.get("config", {})), row["seed"])
            if row.get("position") is not None:
                env.reset(row["seed"], row["position"])
        else:
            snapshot = row["checkpoint"]
            env = NativeEnv(snapshot["game_id"], json.dumps(snapshot["config"]))
            env.set_state(json.dumps(snapshot))
        rewards = [0.0] * env.num_players
        if row["op"] == "step":
            rewards, _, _ = env.step(row["seat"], row["action"])
        elif row["op"] == "reset":
            env.reset(row["seed"], row.get("position"))
        elif row["op"] == "observe" and state.fail_observe:
            return {"status": "error", "error": {"code": "ENGINE_UNAVAILABLE", "message": "private-server-detail"}}
        seat = row.get("seat")
        if seat is None:
            observation = {"text": env.public_text(), "json": {}}
            legal, mask = [], [False] * env.action_space_size
        else:
            frame = env.frame(seat)
            observation = {"text": frame["text"], "json": json.loads(frame["board_json"]),
                           "tensor": {"shape": list(frame["observation"].shape),
                                      "values": frame["observation"].reshape(-1).tolist()}}
            legal = [{"index": index, "string": text} for index, text in frame["legal_actions"]]
            mask = [bool(value) for value in frame["action_mask"]]
        terminated, truncated = env.flags()
        return {"status": "ok", "checkpoint": json.loads(env.get_state()), "frame": {
            "seat": seat, "observation": observation, "legal_actions": legal, "action_mask": mask,
            "to_act": env.current_players(), "returns": env.returns(), "rewards": rewards,
            "terminated": terminated, "truncated": truncated}}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def reply(self, status, value):
            raw = json.dumps(value).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        def do_GET(self):
            state.requests.append((self.path, self.headers.get("Authorization"), None))
            if self.path.startswith("/redirect/"):
                self.send_response(302)
                self.send_header("Location", "/v1/games")
                self.end_headers()
            elif self.path == "/v1/games":
                self.reply(200, [json.loads(NativeEnv(game).spec_json()) for game in games()])
            else:
                game = unquote(self.path.rsplit("/", 1)[-1])
                self.reply(200, json.loads(NativeEnv(game).spec_json()))

        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            state.requests.append((self.path, self.headers.get("Authorization"), request))
            if state.response is not None:
                self.reply(*state.response)
                return
            if urlsplit(self.path).path == "/v1/analysis":
                env = NativeEnv(request["game_id"], json.dumps(request["config"]))
                snapshot = json.loads(env.get_state())
                snapshot["state"] = request["from"]["state"]
                env.set_state(json.dumps(snapshot))
                seat = int(urlsplit(self.path).query.split("=")[1])
                index = env.builtin_action(seat, request["opponent"]["id"],
                                           request["opponent"]["level"], request["seed"])
                self.reply(200, {"best_moves": [{"index": index}]})
                return
            results = []
            for row in request["operations"]:
                try:
                    results.append(operation(row))
                except ValueError:
                    results.append({"status": "error", "error": {"code": "ILLEGAL_ACTION"}})
            if state.malformed:
                results[0]["frame"]["observation"]["tensor"]["shape"] = [1000000000000]
            self.reply(200, {"results": results})

    httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=lambda: httpd.serve_forever(poll_interval=0.01), daemon=True)
    worker.start()
    try:
        yield f"http://127.0.0.1:{httpd.server_port}", state
    finally:
        httpd.shutdown()
        httpd.server_close()
        worker.join(timeout=2)


def equal_frame(left, right):
    np.testing.assert_array_equal(left["observation"], right["observation"])
    np.testing.assert_array_equal(left["action_mask"], right["action_mask"])
    assert left["text"] == right["text"]
    assert json.loads(left["board_json"]) == json.loads(right["board_json"])
    assert left["legal_actions"] == right["legal_actions"]


def test_remote_native_matches_local_through_a_full_game_and_checkpoint(server):
    url, _ = server
    client = connect(url, "test-token")
    assert client.games() == games()
    remote, local = client.native("tictactoe", seed=42), NativeEnv("tictactoe", seed=42)
    for turn, action in enumerate([0, 3, 1, 4, 2]):
        for seat in range(2):
            equal_frame(remote.frame(seat), local.frame(seat))
        assert remote.step(turn % 2, action) == local.step(turn % 2, action)
        assert json.loads(remote.get_state()) == json.loads(local.get_state())
    assert remote.flags() == (True, False)
    assert remote.returns() == [1.0, -1.0]
    assert remote.public_text() == local.public_text()
    remote.reset(43)
    cloned = remote.clone()
    cloned.step_string(0, "r2c2")
    assert cloned.get_state() != remote.get_state()
    remote.set_state(cloned.get_state())
    assert remote.get_state() == cloned.get_state()


def test_remote_gym_matches_local_random_exchange_and_restore(server):
    url, _ = server
    remote = connect(url).make("connect4", seat=1)
    local = make("connect4", seat=1)
    for env in (remote, local):
        env.reset(seed=19)
    checkpoint = remote.get_state()
    for _ in range(10):
        np.testing.assert_array_equal(remote.action_masks(), local.action_masks())
        action = int(np.flatnonzero(local.action_masks())[0])
        left, right = remote.step(action), local.step(action)
        np.testing.assert_array_equal(left[0], right[0])
        assert left[1:4] == right[1:4]
        if left[2] or left[3]:
            break
    remote.set_state(checkpoint)
    assert remote.get_state() == checkpoint


def test_remote_aec_terminal_rewards_and_public_render(server):
    url, _ = server
    env = connect(url).aec("tictactoe")
    env.reset(seed=0)
    saved = env.get_state()
    env.step(0)
    env.set_state(saved)
    for action in [0, 3, 1, 4, 2]:
        env.step(action)
    assert all(env.terminations.values())
    assert env.rewards == {"player_0": 1.0, "player_1": -1.0}
    assert isinstance(env.render(), str)
    while env.agents:
        env.step(None)


def test_remote_failure_during_frame_refresh_rolls_back_whole_exchange(server):
    url, state = server
    env = connect(url).make("tictactoe")
    env.reset(seed=42)
    before = env.get_state()
    state.fail_observe = True
    with pytest.raises(RemoteError, match="ENGINE_UNAVAILABLE"):
        env.step(0)
    assert env.get_state() == before
    state.fail_observe = False
    state.malformed = True
    with pytest.raises(RemoteError, match="INVALID_RESPONSE"):
        env.step(0)
    assert env.get_state() == before


def test_observation_arrays_are_owned_and_native_search_returns_legal_index(server):
    url, _ = server
    env = connect(url).native("tictactoe")
    frame = env.frame(0)
    frame["action_mask"][:] = 0
    frame["observation"][:] = 100
    assert env.action_mask(0).all()
    assert not np.any(env.frame(0)["observation"] == 100)
    action = env.builtin_action(0, "minimax", 3, 42)
    assert env.action_mask(0)[action]


def test_no_redirect_or_secret_echo_and_bounded_requests(server):
    url, state = server
    client = connect(url, "private-api-key")
    assert "private-api-key" not in repr(client)
    assert client.games()
    assert state.requests[-1][1] == "Bearer private-api-key"
    before = len(state.requests)
    with pytest.raises(RemoteError):
        connect(url + "/redirect", "private-api-key").games()
    assert len(state.requests) == before + 1
    state.response = (401, {"error": {"code": "UNAUTHORIZED", "message": "private-api-key"}})
    with pytest.raises(RemoteError) as error:
        client.batch([{"op": "observe"}])
    assert str(error.value) == "remote environment failed: UNAUTHORIZED"
    assert error.value.status == 401
    with pytest.raises(RemoteError, match="REQUEST_TOO_LARGE"):
        client.batch([{"padding": "x" * (4 * 1024 * 1024)}])
    with pytest.raises(ValueError):
        client.batch([])
    for address in ["file:///tmp/key", "http://user:pass@localhost", url + "?key=secret"]:
        with pytest.raises(ValueError):
            connect(address)


def test_error_rows_do_not_mutate_siblings_or_bad_checkpoint_target(server):
    url, _ = server
    client = connect(url)
    env = client.native("tictactoe")
    snapshot = json.loads(env.get_state())
    results = client.batch([
        {"op": "step", "checkpoint": snapshot, "seat": 0, "action": 4},
        {"op": "step", "checkpoint": snapshot, "seat": 1, "action": 4},
    ])
    assert [row["status"] for row in results] == ["ok", "error"]
    assert json.loads(env.get_state()) == snapshot
    bad = copy.deepcopy(snapshot)
    bad["game_id"] = "connect4"
    with pytest.raises(ValueError):
        env.set_state(json.dumps(bad))
    assert json.loads(env.get_state()) == snapshot
