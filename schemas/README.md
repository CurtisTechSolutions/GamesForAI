# Generated API contracts

The runtime contract is at `GET /v1/openapi.json`. Export the same OpenAPI 3.1
document with:

```sh
cargo run -p gfa-http --example export-openapi --locked > schemas/openapi.json
```

Schema derives are optional features of `gfa-core` and `gfa-api-types`; pure
engine consumers do not need OpenAPI support. The HTTP adapter enables the
features and registers the actual handlers. CI exports the contract as an
artifact. Do not maintain a second hand-written schema.

## Browser DTOs

`web/packages/api-client/src/generated.ts` is generated from the installed Rust OpenAPI components, including tagged unions and nullability. Web CI rebuilds the document and rejects drift. Regenerate with:

```sh
cargo run -p gfa-http --example export-openapi --locked > /tmp/gfa-openapi.json
python3 .github/scripts/generate-api-types.py /tmp/gfa-openapi.json web/packages/api-client/src/generated.ts
```

The generator emits transport types, not runtime validation or game rules. Its fixture tests cover references, optional/nullable fields, tagged unions, arrays and maps. The API client is the sole browser network boundary.
