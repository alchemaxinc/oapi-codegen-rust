# Design decisions vs. Go

Rust differs enough from Go that a faithful port makes different choices in
places. This lists where `oapi-codegen-rust` deviates from
[`oapi-codegen`](https://github.com/oapi-codegen/oapi-codegen) and why.

## Server: native async trait, axum only

- **Go:** generates for many frameworks (chi, echo, gin, fiber, gorilla, iris,
  stdhttp) from a plain-method interface.
- **Rust:** one `trait Api` using native async-fn-in-traits
  (`fn op(&self, ..) -> impl Future<Output = ..> + Send`), targeting axum 0.8.
  No `async-trait` dependency.
- **Why:** stable Rust does async-in-traits natively; one well-supported
  framework keeps output idiomatic without a macro dependency.
- There is a single server mode — a typed `trait Api`, comparable to Go's
  strict server. There is no unstructured handler variant.

## Typed status-code response enums

- **Go:** per-status response structs / context returns.
- **Rust:** one `enum <Op>Response` per operation with a variant per declared
  status. The same enum implements `IntoResponse` on the server and is the
  client's success type, so a new status is a compile-enforced exhaustive match
  on both sides.

## Token-based generation, not templates

- **Go:** user-overridable `text/template`.
- **Rust:** build a `TokenStream` with `quote!`, parse to `syn::File` (which
  guarantees syntactically valid Rust), and pretty-print with `prettyplease`.

## Blocking `reqwest` client

- Returns `Result<<Op>Response, ClientError>` with a single hand-written
  `ClientError` enum. Blocking by default, so no async runtime is forced on
  consumers.

## Derive HTTP status semantics

- Status codes come from the `http` / `axum` / `reqwest` `StatusCode` types
  rather than a hand-maintained reason/table mirror.

## Vendor extensions keyed `x-rust-*`

- `x-rust-type`, `x-rust-name`, `x-rust-serde-skip` — the Go-named keys are not
  accepted. See [extensions](extensions.md).

## Combined output via submodules

- Emitting server and client together puts shared component models at the crate
  root and the two targets in `server` / `client` submodules, resolving the
  same-named/different-shaped per-operation items.

## OpenAPI 3.0

- The generator reads OpenAPI 3.0 documents (via the `openapiv3` crate).

## Config compatibility

- The YAML config mirrors `oapi-codegen`'s keys, and unknown keys are ignored,
  so an existing Go config can be reused as-is. See
  [configuration](configuration.md).
