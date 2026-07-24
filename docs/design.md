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

## Direction-aware serde derives

- **Go:** no derives — `encoding/json` reflects over structs, so a type
  round-trips in both directions with zero annotations.
- **Rust:** a generated model derives `serde::Serialize` and/or
  `serde::Deserialize` only for the directions the API actually uses it in. A
  server serializes response bodies and deserializes request bodies; a client
  does the reverse. A model reachable only as a response on a server-only
  generation derives `Serialize` (not `Deserialize`), and vice versa; a model
  used in both directions — or any model when both a server and a client are
  generated — derives both, matching the previous unconditional behaviour.
- **Why:** deriving a serde trait a type never needs would impose an
  unsatisfiable bound on a reused `x-rust-type` target — e.g. forcing
  `Deserialize` on a response-only type a project only ever serializes. The
  usage direction is computed by walking the operations and propagating through
  inter-model references (`emit/usage.rs`). `Debug`, `Clone`, and `PartialEq`
  are still derived unconditionally.

## Vendor extensions

- `x-rust-type`, `x-rust-name`, `x-rust-serde-skip`, plus `oapi-codegen`
  compatibility keys (`x-omitempty`, `x-order`, `x-deprecated-reason`,
  `x-enum-varnames` / `x-enumNames`).
- `x-go-*` keys are ignored (accepted, no effect). See
  [extensions](extensions.md).

## Combined output at the crate root

- Models, per-operation types (response enums, parameter structs, request/
  response body enums), and both the server and client interfaces are emitted
  flat at the crate root. Server and client share one file and the same
  per-operation types, so a consumer refers to them directly (e.g.
  `use api::GetPetResponse`).
- A generated per-operation type whose name matches an emitted component model
  (most commonly a schema named `<Op>Response`) fails generation rather than
  renaming either item silently. Resolve the clash by renaming the schema with
  `x-rust-name`, or, for a response-enum clash, set
  `output-options.response-type-suffix` to move the enum aside
  (`GetWidgetResponse` → `GetWidgetResp`) while the schema keeps its name.

## OpenAPI 3.0

- The generator reads OpenAPI 3.0 documents (via the `openapiv3` crate).

## Config compatibility

- The YAML config mirrors `oapi-codegen`'s keys, and unknown keys are ignored,
  so an existing Go config can be reused as-is. See
  [configuration](configuration.md).

## Explicit over implicit invocation

- **Go:** `oapi-codegen` runs without a config file — `-generate` defaults to
  `types,client,server,spec` and `-o` defaults to stdout — so a bare
  `oapi-codegen spec.yaml` produces output.
- **Rust:** invocation is explicit. `--config-file` is required and must enable
  at least one artifact, an output destination is required (via `--output-file`
  or the config's `output:` key), and generation that would emit no code fails
  with a guided explanation instead of writing an empty file.
- **Why:** the CLI is a build step whose behavior should be obvious from the
  command and config, not from defaults; failing loudly with next steps beats a
  silent empty file.
