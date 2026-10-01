"""Shared reset selection; puzzle answers never enter policy observations."""
from .positions import PositionSet


def resolve_start(native, options, rng, previous):
    if "position" in options and "position_set" in options:
        raise ValueError("choose position or position_set, not both")
    position = options.get("position")
    if position is not None and not isinstance(position, str):
        raise ValueError("position must be a notation string")
    dataset = None if "position" in options else previous
    if "position_set" in options:
        dataset = options["position_set"]
        if isinstance(dataset, str):
            dataset = PositionSet.load(dataset)
        if dataset is not None and not isinstance(dataset, PositionSet):
            raise ValueError("position_set must be name@version, a PositionSet, or None")
    if dataset is not None:
        dataset.validate_for(native)
        entry = dataset.sample(rng)
        return entry.position, dataset, entry.id
    return position, None, None


def origin_info(dataset, position_id):
    if dataset is None:
        return {}
    return {"curriculum": {
        "position_set": dataset.identifier,
        "sha256": dataset.sha256,
        "position_id": position_id,
    }}


def checkpoint(dataset, position_id):
    if dataset is None:
        return None
    return {**dataset.checkpoint(), "position_id": position_id}


def restore(payload, native):
    if payload is None:
        return None, None
    if not isinstance(payload, dict) or set(payload) != {"manifest", "data", "position_id"}:
        raise ValueError("invalid curriculum checkpoint")
    dataset = PositionSet.from_jsonl(payload["manifest"], payload["data"])
    dataset.validate_for(native)
    if payload["position_id"] not in {entry.id for entry in dataset.entries}:
        raise ValueError("curriculum checkpoint position is not in the pinned set")
    return dataset, payload["position_id"]
