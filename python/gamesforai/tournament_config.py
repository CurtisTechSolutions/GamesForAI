"""Bounded, data-only tournament configuration and explicit policy imports."""
from __future__ import annotations

import importlib
import json
import re
from pathlib import Path

from .http_policy import ChatPolicy
from .ratings import Rating
from .tournament import Agent


class ConfigurationError(ValueError):
    """An actionable message that never includes configuration values."""


def load_document(path):
    try:
        import yaml
    except ImportError as error:
        raise ConfigurationError("install gamesforai[tournaments] to read agent/config files") from error

    class StrictLoader(yaml.SafeLoader):
        pass

    def mapping(loader, node):
        result = {}
        for key_node, value_node in node.value:
            key = loader.construct_object(key_node)
            if not isinstance(key, str) or key in result:
                raise ConfigurationError("configuration keys must be unique strings")
            result[key] = loader.construct_object(value_node)
        return result

    StrictLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, mapping)
    try:
        with Path(path).open("rb") as stream:
            raw = stream.read(65537)
        if len(raw) > 65536:
            raise ConfigurationError("configuration exceeds 64 KiB")
        text = raw.decode("utf-8")
        depth = 0
        for count, event in enumerate(yaml.parse(text), 1):
            if isinstance(event, yaml.events.AliasEvent):
                raise ConfigurationError("YAML aliases are not supported")
            if isinstance(event, (yaml.events.MappingStartEvent, yaml.events.SequenceStartEvent)):
                depth += 1
            elif isinstance(event, (yaml.events.MappingEndEvent, yaml.events.SequenceEndEvent)):
                depth -= 1
            if depth > 32 or count > 8192:
                raise ConfigurationError("configuration is too deeply nested or complex")
        result = yaml.load(text, Loader=StrictLoader)
        if not isinstance(result, dict):
            raise ConfigurationError("configuration must be an object")
        json.dumps(result, allow_nan=False)
        return result
    except ConfigurationError:
        raise
    except Exception as error:
        raise ConfigurationError("cannot read configuration as finite JSON or safe YAML") from error


def opponent_names(text):
    """Expand a comma-separated native ladder without guessing calibrated ratings."""
    names = []
    for token in text.split(","):
        token = token.strip()
        match = re.fullmatch(r"(minimax|mcts|stockfish):([1-9]|10)(?:\.\.([1-9]|10))?", token)
        if token == "random":
            expanded = [token]
        elif match:
            algorithm, lower, upper = match.groups()
            lower, upper = int(lower), int(upper or lower)
            if lower > upper:
                raise ConfigurationError("opponent ranges must ascend within levels 1..10")
            expanded = [f"{algorithm}:{level}" for level in range(lower, upper + 1)]
        else:
            raise ConfigurationError("opponents must be random or minimax/mcts/stockfish:1..10")
        names.extend(expanded)
    if len(set(names)) != len(names):
        raise ConfigurationError("opponent selections must be unique")
    return names


def load_agent(document, pool):
    """Python factory imports are caller-selected executable model code."""
    common = {"id", "type", "initial_rating", "fixed"}
    kind = document.get("type")
    options = {
        "builtin": {"opponent"},
        "python": {"factory", "params"},
        "chat": {"base_url", "model", "api_key_env", "timeout", "max_tokens", "temperature", "structured"},
    }
    if not isinstance(kind, str) or kind not in options or set(document) - common - options[kind]:
        raise ConfigurationError("unknown agent type or configuration field")
    if "id" not in document:
        raise ConfigurationError("every agent requires a frozen snapshot id")
    arguments = {"id": document["id"], "fixed": document.get("fixed", False)}
    try:
        if "initial_rating" in document:
            arguments["initial_rating"] = Rating(**document["initial_rating"])
        if kind == "builtin":
            opponent = document["opponent"]
            arguments["opponent"] = opponent
            if isinstance(opponent, str) and opponent.startswith("stockfish:"):
                arguments["stockfish_pool"] = pool()
        elif kind == "chat":
            settings = {key: value for key, value in document.items() if key in options[kind]}
            ChatPolicy(**settings)  # Validate without contacting the model.
            arguments["policy_factory"] = lambda seed, seat: ChatPolicy(**settings)
        else:
            target = document["factory"]
            if not isinstance(target, str) or not re.fullmatch(
                r"[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*:[A-Za-z_]\w*", target
            ):
                raise ConfigurationError("Python factory must be an importable module:callable")
            params = document.get("params", {})
            if not isinstance(params, dict) or {"seed", "seat"} & params.keys():
                raise ConfigurationError("factory params must be an object without seed/seat overrides")
            module, name = target.split(":")
            factory = getattr(importlib.import_module(module), name)
            if not callable(factory):
                raise ConfigurationError("Python factory must be callable")
            arguments["policy_factory"] = lambda seed, seat: factory(seed=seed, seat=seat, **params)
        return Agent(**arguments)
    except ConfigurationError:
        raise
    except Exception as error:
        raise ConfigurationError("agent fields, rating, Python factory or engine configuration are invalid") from error
