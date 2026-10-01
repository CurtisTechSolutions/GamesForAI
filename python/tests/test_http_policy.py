import json
import threading
import time
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from gamesforai import ChatPolicy, PolicyError, make


@contextmanager
def server(reply=None, status=200, delay=0):
    calls = []
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            calls.append((self.path, dict(self.headers), body))
            time.sleep(delay)
            self.send_response(status)
            if status == 307:
                self.send_header("Location", "http://127.0.0.1:1/should-not-follow")
            self.end_headers()
            if reply is None:
                legal = json.loads(body["messages"][1]["content"])["legal_actions"]
                value = {"choices": [{"finish_reason": "stop", "message": {"content": json.dumps({"action": legal[0]})}}]}
                data = json.dumps(value).encode()
            else:
                data = reply
            try:
                self.wfile.write(data)
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                pass

    httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=httpd.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{httpd.server_port}/v1", calls
    finally:
        httpd.shutdown()
        httpd.server_close()
        thread.join(timeout=2)


def frame():
    env = make("connect4")
    return env.reset(seed=1)


def test_own_model_endpoint_receives_only_policy_view(monkeypatch):
    monkeypatch.setenv("GFA_TEST_MODEL_KEY", "test-only-credential")
    obs, info = frame()
    info["checkpoint"] = {"secret": "PRIVATE_STATE"}
    info["expected"] = "PUZZLE_ANSWER"
    with server() as (url, calls):
        policy = ChatPolicy(url, "my-local-model", api_key_env="GFA_TEST_MODEL_KEY")
        assert policy(obs, info) == 0
        assert len(calls) == 1
        path, headers, body = calls[0]
        assert path == "/v1/chat/completions"
        assert headers["Authorization"] == "Bearer test-only-credential"
        assert body["model"] == "my-local-model"
        assert body["response_format"]["json_schema"]["schema"]["properties"]["action"]["enum"] == list("1234567")
        encoded = json.dumps(body)
        assert "PRIVATE_STATE" not in encoded and "PUZZLE_ANSWER" not in encoded
        assert "test-only-credential" not in repr(policy)
        assert "test-only-credential" not in encoded


@pytest.mark.parametrize("status,code", [(401, "unauthorized"), (429, "rate_limited"), (500, "http_error"), (307, "http_error")])
def test_http_errors_and_redirects_are_not_retried_or_exposed(status, code):
    with server(b"private error body", status) as (url, calls):
        with pytest.raises(PolicyError) as error:
            ChatPolicy(url, "own-model")(*frame())
        assert error.value.code == code
        assert "private error body" not in str(error.value)
        assert len(calls) == 1


@pytest.mark.parametrize("reply,code", [
    (b"invalid JSON", "invalid_response"),
    (json.dumps({"choices": [{"finish_reason": "length", "message": {"content": '{"action":"1"}'}}]}).encode(), "invalid_response"),
    (json.dumps({"choices": [{"finish_reason": "stop", "message": {"content": '{"action":"99"}'}}]}).encode(), "illegal_action"),
    (json.dumps({"choices": [{"finish_reason": "stop", "message": {"content": '{"action":"1","action":"2"}'}}]}).encode(), "invalid_response"),
    (b"x" * 65537, "response_too_large"),
])
def test_response_validation_and_gym_exchange_rollback(reply, code):
    with server(reply) as (url, calls):
        env = make("connect4", opponent=ChatPolicy(url, "own-model"))
        env.reset(seed=4)
        before = env.get_state()
        with pytest.raises(PolicyError) as error:
            env.step(3)
        assert error.value.code == code
        assert env.get_state() == before
        assert len(calls) == 1


def test_timeout_missing_key_and_plain_json_compatibility(monkeypatch):
    monkeypatch.delenv("GFA_TEST_MISSING_KEY", raising=False)
    with server(delay=0.2) as (url, calls):
        with pytest.raises(PolicyError) as error:
            ChatPolicy(url, "own-model", timeout=0.02)(*frame())
        assert error.value.code == "timeout"
    with server() as (url, calls):
        with pytest.raises(PolicyError) as error:
            ChatPolicy(url, "own-model", api_key_env="GFA_TEST_MISSING_KEY")(*frame())
        assert error.value.code == "missing_credential" and not calls
        assert ChatPolicy(url, "own-model", structured=False)(*frame()) == 0
        assert "response_format" not in calls[0][2]


def test_full_episode_against_model_endpoint():
    with server() as (url, calls):
        env = make("tictactoe", opponent=ChatPolicy(url, "own-model"))
        _, info = env.reset(seed=0)
        for _ in range(5):
            _, reward, terminated, truncated, info = env.step(info["legal_actions"][-1][0])
            if terminated or truncated:
                break
        else:
            pytest.fail("game did not finish")
        assert calls
        assert all(json.loads(call[2]["messages"][1]["content"])["seat"] == 1 for call in calls)
