# oapi-codegen-rust

Generate idiomatic Rust API and Clients from OpenAPI 3 specifications, inspired by
[`oapi-codegen`](https://github.com/oapi-codegen/oapi-codegen).

## Justification

This project aims to fill a gap that does not seem to have an established solution in the Rust ecosystem: Generating API
servers and client boilerplate directly from OpenAPI specifications.

The project takes inspiration from [`oapi-codegen`](https://github.com/oapi-codegen/oapi-codegen) and aims to provide a
similar experience for Rust developers. Since Rust has substantial differences from Go, some key design decisions
differs:

### Design decisions differences

* `axum` only, no generation for multiple web frameworks.
* Typed status-code response enums (shared IR) instead of response structs.
* Token-based generation, not text templates (Go uses `text/template`, we use a custom token-based approach).
* `reqwest` client generation only, biased towards blocking for simplicity, no async runtime forced on consumers.
* Vendor extensions keyed `x-rust-*`, and serde attributes, Go keys rejected.

Otherwise, the project strives to be a faithful drop-in from the Go-based version in as idiomatic Rust as possible, with
similar command-line interface and configuration options.

