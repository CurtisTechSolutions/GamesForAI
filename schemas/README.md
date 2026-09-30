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
