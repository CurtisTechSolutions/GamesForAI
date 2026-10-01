"""Generate TypeScript DTOs from the installed Rust OpenAPI components."""
import argparse
import json
from pathlib import Path
import sys


def literal(value):
    return json.dumps(value, ensure_ascii=True, separators=(",", ":"))


def schema_type(schema):
    if schema is True:
        return "JsonValue"
    if schema is False:
        return "never"
    if not isinstance(schema, dict):
        raise ValueError("schema must be an object or boolean")
    if "$ref" in schema:
        reference = schema["$ref"]
        prefix = "#/components/schemas/"
        if not reference.startswith(prefix) or "/" in reference[len(prefix):]:
            raise ValueError("only local component references are supported")
        return f"Schemas[{literal(reference[len(prefix):])}]"
    if "const" in schema:
        return literal(schema["const"])
    if "enum" in schema:
        return " | ".join(literal(value) for value in schema["enum"]) or "never"
    for keyword, join in [("oneOf", " | "), ("anyOf", " | "), ("allOf", " & ")]:
        if keyword in schema:
            return join.join(f"({schema_type(value)})" for value in schema[keyword])
    kind = schema.get("type")
    if isinstance(kind, list):
        return " | ".join(schema_type({**schema, "type": item}) for item in kind)
    if kind == "null":
        return "null"
    if kind in ("integer", "number"):
        return "number"
    if kind in ("string", "boolean"):
        return kind
    if kind == "array" or "items" in schema:
        if "prefixItems" in schema:
            return "[" + ", ".join(schema_type(item) for item in schema["prefixItems"]) + "]"
        return f"Array<{schema_type(schema.get('items', {}))}>"
    if kind == "object" or "properties" in schema:
        properties = schema.get("properties", {})
        required = set(schema.get("required", []))
        fields = [f"{literal(name)}{'' if name in required else '?'}: {schema_type(value)};"
                  for name, value in sorted(properties.items())]
        result = "{ " + " ".join(fields) + " }"
        additional = schema.get("additionalProperties")
        if additional is not None and additional is not False:
            extra = f"Record<string, {schema_type(additional)}>"
            return f"({result}) & {extra}" if properties else extra
        return result if properties else ("Record<string, never>" if additional is False else "Record<string, JsonValue>")
    return "JsonValue"


def generate(document):
    schemas = document["components"]["schemas"]
    names = set(schemas)
    def check(value):
        if isinstance(value, dict):
            if "$ref" in value:
                reference = value["$ref"]
                if reference.removeprefix("#/components/schemas/") not in names:
                    raise ValueError("unresolved component reference")
            for item in value.values():
                check(item)
        elif isinstance(value, list):
            for item in value:
                check(item)
    check(schemas)
    lines = [
        "// Generated from the Rust OpenAPI document. Do not edit by hand.",
        "// Regenerate with .github/scripts/generate-api-types.py.",
        "// prettier-ignore",
        "export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };",
        "",
        "// prettier-ignore",
        "export interface Schemas {",
    ]
    for name, schema in sorted(schemas.items()):
        lines.append(f"  {literal(name)}: {schema_type(schema)};")
    return "\n".join([*lines, "}", ""])


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("document")
    parser.add_argument("output")
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    result = generate(json.loads(Path(arguments.document).read_text()))
    output = Path(arguments.output)
    if arguments.check:
        if not output.is_file() or output.read_text() != result:
            # Machine-readable recovery data, also useful when bootstrapping a new DTO.
            print(json.dumps({"generated_path": str(output), "generated_source": result}))
            sys.exit("Generated TypeScript DTOs differ; regenerate from the current OpenAPI document.")
    else:
        output.write_text(result)
