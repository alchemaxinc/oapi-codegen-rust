# Dependency report example

Cargo does not infer crate dependencies from a generated file's `use` paths the
way Go's `go mod tidy` does, so `oapi-codegen` reports the crates a generated
file needs that the target package's `Cargo.toml` does not yet declare — as both
a `Cargo.toml` snippet and `cargo add` commands.

This crate deliberately starts with no HTTP dependencies (and does not compile
the generated file), so generating a server here shows the full report. Only
crates missing from the nearest `Cargo.toml` are listed, so once you add them the
report goes quiet.

```console
$ oapi-codegen --config-file oapi-codegen.yaml openapi.yaml
✓ wrote generated/api.rs
  note: add the crates the generated code references to Cargo.toml:
      serde = { version = "1.0.229", features = ["derive"] }
      http = "1.4.2"
      axum = "0.8.9"
      axum-extra = { version = "0.12.6", features = ["query"] }
      # or:
      cargo add serde@1.0.229 --features derive
      cargo add http@1.4.2
      cargo add axum@0.8.9
      cargo add axum-extra@0.12.6 --features query

```

Pass `--install-deps` to run the `cargo add` commands automatically (on an
interactive terminal you are asked first).
