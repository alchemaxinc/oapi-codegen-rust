# Bookstore Example

Regenerate this example from the multi-file OpenAPI spec.
Run each command from this directory.

```console
$ oapi-codegen --config-file oapi-codegen-common.yaml schemas/common.yaml
✓ wrote generated/apimodel/common.rs
  note: add the crates the generated code references to Cargo.toml:
      serde = { version = "1.0.229", features = ["derive"] }
      # or:
      cargo add serde@1.0.229 --features derive

$ oapi-codegen --config-file oapi-codegen-catalog.yaml schemas/catalog.yaml
✓ wrote generated/apimodel/catalog.rs
  note: add the crates the generated code references to Cargo.toml:
      serde = { version = "1.0.229", features = ["derive"] }
      # or:
      cargo add serde@1.0.229 --features derive

$ oapi-codegen --config-file oapi-codegen-server.yaml openapi.yaml
✓ wrote generated/restapi.rs and 15 modules beside it
  note: add the crates the generated code references to Cargo.toml:
      serde = { version = "1.0.229", features = ["derive"] }
      serde_json = "1.0.151"
      http = "1.5.0"
      axum = { version = "0.8.9", features = ["multipart"] }
      axum-extra = { version = "0.12.6", features = ["query"] }
      # or:
      cargo add serde@1.0.229 --features derive
      cargo add serde_json@1.0.151
      cargo add http@1.5.0
      cargo add axum@0.8.9 --features multipart
      cargo add axum-extra@0.12.6 --features query

$ oapi-codegen --config-file oapi-codegen-client.yaml openapi.yaml
✓ wrote generated/restclient.rs and 14 modules beside it
  note: add the crates the generated code references to Cargo.toml:
      serde_json = "1.0.151"
      http = "1.5.0"
      reqwest = { version = "0.13.4", default-features = false, features = ["blocking", "json", "form", "query", "multipart"] }
      percent-encoding = "2.3.2"
      # or:
      cargo add serde_json@1.0.151
      cargo add http@1.5.0
      cargo add reqwest@0.13.4 --no-default-features --features blocking,json,form,query,multipart
      cargo add percent-encoding@2.3.2

```

Each config file sets its own `output:` value, so `--output-file` is not needed.
From the repository root, `make generate-example` runs the same commands.
