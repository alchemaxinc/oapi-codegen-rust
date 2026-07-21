# oapi-codegen-rust

Generate idiomatic Rust APIs and clients from OpenAPI 3 specifications, inspired by
[`oapi-codegen`](https://github.com/oapi-codegen/oapi-codegen).

## Quickstart

```sh
cargo install oapi-codegen
```

```console
$ oapi-codegen --config-file cfg.yaml --output-file src/api.rs openapi.yaml
? failed
error: failed to read config file `cfg.yaml`: No such file or directory (os error 2)
  hint: No config file exists at `cfg.yaml`; check the path and your working directory.

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
- Targets OpenAPI 3.0.
- Reuses your `oapi-codegen` YAML config — unknown keys are ignored.
- Explicit over implicit: `--config-file` and an output path are required, and
  empty generation fails loudly (Go defaults these and prints to stdout).
- `x-rust-*` vendor extensions; `x-go-*` keys ignored.

See [design decisions vs. Go](docs/design.md) for the rationale.

Otherwise, the project strives to be a faithful drop-in from the Go-based version in as idiomatic Rust as possible, with
similar command-line interface and configuration options.

## Documentation

- [Installation](docs/installation.md)
- [Configuration](docs/configuration.md)
- [OpenAPI extensions](docs/extensions.md)
- [Design decisions vs. Go](docs/design.md)
