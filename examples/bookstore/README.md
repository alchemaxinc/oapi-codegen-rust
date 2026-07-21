# Bookstore Example

Regenerate this example from its multi-file OpenAPI spec by running the CLI once
per config, from this directory:

```sh
oapi-codegen --config-file oapi-codegen-common.yaml schemas/common.yaml
oapi-codegen --config-file oapi-codegen-catalog.yaml schemas/catalog.yaml
oapi-codegen --config-file oapi-codegen-server.yaml openapi.yaml
oapi-codegen --config-file oapi-codegen-client.yaml openapi.yaml
```

Each config sets its own `output:`, so no `--output-file` is needed. From the
repository root, `make generate-example` runs the same commands.
