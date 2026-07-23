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
- [Configuration](docs/configuration.md)
- [OpenAPI extensions](docs/extensions.md)
- [Design decisions vs. Go](docs/design.md)
