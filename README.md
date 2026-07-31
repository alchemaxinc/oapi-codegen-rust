# oapi-codegen-rust

Generate idiomatic Rust APIs and clients from OpenAPI 3 specifications, inspired by
[`oapi-codegen`](https://github.com/oapi-codegen/oapi-codegen).

## Quickstart

```sh
cargo install oapi-codegen
```

From this repository's root (e.g., after cloning), try the included bookstore example:

```console
$ oapi-codegen --config-file examples/bookstore/oapi-codegen-server.yaml examples/bookstore/openapi.yaml
✓ wrote generated/restapi.rs
  note: add the crates the generated code references to Cargo.toml:
      serde = { version = "1.0.229", features = ["derive"] }
      serde_json = "1.0.151"
      http = "1.4.2"
      axum = { version = "0.8.9", features = ["multipart"] }
      axum-extra = { version = "0.12.6", features = ["query"] }
      # or:
      cargo add serde@1.0.229 --features derive
      cargo add serde_json@1.0.151
      cargo add http@1.4.2
      cargo add axum@0.8.9 --features multipart
      cargo add axum-extra@0.12.6 --features query

```

See [docs/](docs/) for installation, configuration, and extensions.

## Justification

This project aims to fill a gap that does not seem to have an established solution in the Rust ecosystem: Generating API
servers and client boilerplate directly from OpenAPI specifications.

The project takes inspiration from [`oapi-codegen`](https://github.com/oapi-codegen/oapi-codegen) and aims to provide a
similar experience for Rust developers. Since Rust has substantial differences from Go, some key design decisions
differ:

### Design differences

- `axum` only — no multi-framework generation.
- One server mode: a typed `trait Api` (comparable to Go's strict server) — no unstructured handler variant.
- Typed status-code response enums instead of response structs.
- Token-based generation (`quote`/`syn`), not user-overridable `text/template`.
- Blocking `reqwest` client — no async runtime forced on consumers.
- Reuses your `oapi-codegen` YAML config — unknown keys are ignored.
- Explicit over implicit: `--config-file` is required, and an output path must be given via `--output-file` or the
  config's `output:` key; empty generation fails loudly (Go defaults these and prints to stdout).
- Fails fast where Go assumes: where `oapi-codegen` silently defaults, guesses, or ignores an ambiguity, this generator
  stops with a guided error and lets a person decide. For example, two schemas that produce one Rust type name are an
  error, and Go writes `MyWidget2` instead. A `<Op>Response` enum that clashes with a same-named schema, and a path
  parameter that does not match the path template, are errors for the same reason. The generator surfaces the decision
  and does not bake in an assumption.
- `x-rust-*` vendor extensions; `x-go-*` keys ignored.

See [design decisions vs. Go](docs/design.md) for the rationale.

Otherwise, the project strives to be a faithful drop-in from the Go-based version in as idiomatic Rust as possible, with
similar command-line interface and configuration options.

### OpenAPI Version Support

Adding full support to all OpenAPI versions is not trivial, and is done in iterations which all require various design
decisions to support.

| Version | Status    | Notes                                                                                          |
| ------- | --------- | ---------------------------------------------------------------------------------------------- |
| v3.0.0  | Supported | Supported in [`v1.0.0`](https://github.com/alchemaxinc/oapi-codegen-rust/releases/tag/v1.0.0). |
| v3.1.0  | Planned   | Tracking [here](https://github.com/alchemaxinc/oapi-codegen-rust/issues/65).                   |
| v3.2.0  | Planned   | Tracking [here](https://github.com/alchemaxinc/oapi-codegen-rust/issues/66).                   |

This table and the tracking issues are maintained automatically by the
[OpenAPI version check workflow](.github/workflows/check-openapi-versions.yml),
which probes the [upstream spec releases](https://github.com/OAI/OpenAPI-Specification/releases)
against [`.github/openapi-versions.json`](.github/openapi-versions.json).

## Documentation

> [!NOTE]  
> These documents are primarily written by LLMs and are not intended for human reading. This project aims to be self-explanatory
> through a sharp focus on TUI UX and guided error messaging. The documentation exists to help guide LLM context where needed.
> They have been checked and verified by humans, and can be read if interested.

- [Installation](docs/installation.md)
- [Build workflow](docs/workflow.md)
- [Configuration](docs/configuration.md)
- [OpenAPI extensions](docs/extensions.md)
- [Design decisions vs. Go](docs/design.md)
