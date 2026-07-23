# Bookstore Example

Regenerate this example from its multi-file OpenAPI spec by running the CLI once
per config, from this directory:

```console
$ oapi-codegen --config-file oapi-codegen-common.yaml schemas/common.yaml
✓ wrote generated/apimodel/common.rs

$ oapi-codegen --config-file oapi-codegen-catalog.yaml schemas/catalog.yaml
✓ wrote generated/apimodel/catalog.rs

$ oapi-codegen --config-file oapi-codegen-server.yaml openapi.yaml
✓ wrote generated/restapi.rs

$ oapi-codegen --config-file oapi-codegen-client.yaml openapi.yaml
✓ wrote generated/restclient.rs

```

Each config sets its own `output:`, so no `--output-file` is needed. From the
repository root, `make generate-example` runs the same commands.
