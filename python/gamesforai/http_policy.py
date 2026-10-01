"""Use a caller-hosted chat-completions model as an ordinary Python policy."""
from __future__ import annotations

import json
import math
import os
import re
import socket
from http.client import HTTPException
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

import numpy as np


class PolicyError(ValueError):
    """Stable failure code without response bodies, credentials, or endpoint URLs."""

    def __init__(self, code):
        self.code = code
        super().__init__(f"model policy failed: {code}")


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, file, code, message, headers, new_url):
        return None


def _json(text):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate JSON key")
            result[key] = value
        return result

    def invalid(value):
        raise ValueError("non-finite JSON number")

    return json.loads(text, object_pairs_hook=pairs, parse_constant=invalid)


class ChatPolicy:
    """Seat-scoped inference via an OpenAI-compatible /chat/completions endpoint.

    Pass the server's API root (normally ending in /v1) and its model identifier.
    This adapter makes one non-streaming request per turn, with no automatic retry.
    """

    def __init__(
        self, base_url, model, *, api_key_env=None, timeout=60.0,
        max_tokens=128, temperature=0.0, structured=True,
    ):
        parsed = urlsplit(base_url)
        if (parsed.scheme not in ("http", "https") or not parsed.hostname
                or parsed.username or parsed.password or parsed.query or parsed.fragment):
            raise ValueError("base_url must be an HTTP(S) API root without credentials or query")
        if not isinstance(model, str) or not model.strip() or len(model) > 256:
            raise ValueError("model must be a nonempty identifier up to 256 characters")
        if api_key_env is not None and not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", api_key_env):
            raise ValueError("api_key_env must name an environment variable")
        if not math.isfinite(timeout) or not 0 < timeout <= 300:
            raise ValueError("timeout must be in (0, 300] seconds")
        if type(max_tokens) is not int or not 1 <= max_tokens <= 4096:
            raise ValueError("max_tokens must be 1..4096")
        if not math.isfinite(temperature) or not 0 <= temperature <= 2:
            raise ValueError("temperature must be 0..2")
        if type(structured) is not bool:
            raise ValueError("structured must be boolean")
        self._endpoint = base_url.rstrip("/") + "/chat/completions"
        self.model, self.api_key_env = model, api_key_env
        self.timeout, self.max_tokens = timeout, max_tokens
        self.temperature, self.structured = temperature, structured

    def __call__(self, observation, info):
        # Deliberate allowlist: checkpoints, puzzle answers, and arbitrary info
        # fields are never included in a request.
        try:
            mask = np.asarray(info["action_mask"])
            if mask.ndim != 1 or not np.all((mask == 0) | (mask == 1)):
                raise ValueError("invalid mask")
            catalog = list(info["legal_actions"])
            actions = {}
            for index, notation in catalog:
                if (isinstance(index, (bool, np.bool_)) or not isinstance(index, (int, np.integer))
                        or not 0 <= index < mask.size or not mask[index]
                        or not isinstance(notation, str) or not notation or notation in actions):
                    raise ValueError("invalid action catalog")
                actions[notation] = int(index)
            if not actions or sorted(actions.values()) != np.flatnonzero(mask).tolist():
                raise ValueError("catalog and mask differ")
            if not all(isinstance(info[key], str) and info[key] for key in ("game_id", "rules", "text", "action_notation")):
                raise ValueError("missing policy metadata")
            seat = info["seat"]
            if type(seat) is not int or seat < 0:
                raise ValueError("invalid seat")
            payload = {
                "model": self.model,
                "messages": [
                    {"role": "system", "content": (
                        "Play the specified game for the acting seat using only the provided view. "
                        'Return exactly one JSON object {"action":"canonical move"} choosing an action '
                        "from legal_actions. Do not include commentary.\n\n" + info["rules"]
                        + "\nAction notation: " + info["action_notation"]
                    )},
                    {"role": "user", "content": json.dumps({
                        "game_id": info["game_id"], "seat": seat, "board": info["text"],
                        "legal_actions": list(actions),
                    }, allow_nan=False)},
                ],
                "temperature": self.temperature, "max_tokens": self.max_tokens, "stream": False,
            }
            if self.structured:
                payload["response_format"] = {"type": "json_schema", "json_schema": {
                    "name": "game_move", "strict": True, "schema": {
                        "type": "object", "properties": {"action": {"type": "string", "enum": list(actions)}},
                        "required": ["action"], "additionalProperties": False,
                    },
                }}
            data = json.dumps(payload, allow_nan=False).encode("utf-8")
            if len(data) > 64 * 1024:
                raise ValueError("prompt exceeds 64 KiB")
        except (KeyError, TypeError, ValueError, OverflowError):
            raise PolicyError("invalid_input") from None
        headers = {"Content-Type": "application/json", "Accept": "application/json"}
        if self.api_key_env:
            key = os.environ.get(self.api_key_env)
            if not key or "\r" in key or "\n" in key:
                raise PolicyError("missing_credential")
            headers["Authorization"] = "Bearer " + key
        request = Request(self._endpoint, data=data, headers=headers, method="POST")
        try:
            with build_opener(_NoRedirect()).open(request, timeout=self.timeout) as response:
                raw = response.read(64 * 1024 + 1)
                if len(raw) > 64 * 1024:
                    raise PolicyError("response_too_large")
        except PolicyError:
            raise
        except HTTPError as error:
            code = {401: "unauthorized", 403: "unauthorized", 408: "timeout", 429: "rate_limited"}.get(error.code, "http_error")
            error.close()
            raise PolicyError(code) from None
        except (TimeoutError, socket.timeout):
            raise PolicyError("timeout") from None
        except URLError as error:
            raise PolicyError("timeout" if isinstance(error.reason, TimeoutError) else "unavailable") from None
        except (OSError, ValueError, HTTPException):
            raise PolicyError("unavailable") from None
        try:
            result = _json(raw.decode("utf-8"))
            choice = result["choices"][0]
            if choice["finish_reason"] != "stop" or choice["message"].get("refusal"):
                raise ValueError("model did not complete a move")
            move = _json(choice["message"]["content"])
            if not isinstance(move, dict) or set(move) != {"action"} or not isinstance(move["action"], str):
                raise ValueError("invalid move schema")
        except (KeyError, IndexError, TypeError, ValueError, UnicodeError, RecursionError):
            raise PolicyError("invalid_response") from None
        if move["action"] not in actions:
            raise PolicyError("illegal_action")
        return actions[move["action"]]
