"""Contract tests for the OpenAPI-to-TypeScript subset emitted by utoipa."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("api_types", Path(__file__).with_name("generate-api-types.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class Types(unittest.TestCase):
    def test_optional_nullable_tagged_and_reference_types(self):
        value = {"type": "object", "properties": {
            "kind": {"const": "step"}, "action": {"type": ["integer", "null"]},
            "frame": {"$ref": "#/components/schemas/Frame"},
        }, "required": ["kind", "frame"], "additionalProperties": False}
        self.assertEqual(module.schema_type(value),
                         '{ "action"?: number | null; "frame": Schemas["Frame"]; "kind": "step"; }')
        self.assertEqual(module.schema_type({"oneOf": [{"type": "string"}, {"type": "number"}]}),
                         "(string) | (number)")
        self.assertEqual(module.schema_type({"type": "array", "items": {"type": "boolean"}}),
                         "Array<boolean>")

    def test_schema_order_is_stable_and_missing_references_fail(self):
        first = {"components": {"schemas": {"A": {"type": "string"}, "B": {"type": "number"}}}}
        second = {"components": {"schemas": {"B": {"type": "number"}, "A": {"type": "string"}}}}
        self.assertEqual(module.generate(first), module.generate(second))
        with self.assertRaises(ValueError):
            module.generate({"components": {"schemas": {"A": {"$ref": "#/components/schemas/Missing"}}}})
        with self.assertRaises(ValueError):
            module.schema_type({"$ref": "https://example.org/types.json"})

    def test_arbitrary_json_and_dictionary_values_stay_typed(self):
        self.assertEqual(module.schema_type({}), "JsonValue")
        self.assertEqual(module.schema_type({"type": "object", "additionalProperties": {"type": "number"}}),
                         "Record<string, number>")
        self.assertEqual(module.schema_type(False), "never")


unittest.main()
