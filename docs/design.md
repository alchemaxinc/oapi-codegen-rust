# Design decisions compared with Go

Rust and Go differ. This project keeps behavior similar to
[`oapi-codegen`](https://github.com/oapi-codegen/oapi-codegen), but some design
choices are different.

## Server: native async trait, axum only

- **Go:** Many framework targets from one plain interface.
- **Rust:** One `trait Api` with native async fn in traits, for axum 0.8.
- **Reason:** Stable Rust supports async traits directly. One framework keeps
  output clear and avoids extra macro dependencies.

There is one structured server mode. There is no unstructured handler mode.

## Typed status-code response enums

- **Go:** Per-status structs or context returns.
- **Rust:** One `enum <Op>Response` per operation, with one variant per status.

The same enum is the server response (`IntoResponse`) and the client success
type. New status codes require explicit match updates.

## Token-based generation, not templates

- **Go:** `text/template`, user override support.
- **Rust:** Build `TokenStream` with `quote!`, parse with `syn::File`, and
  format with `prettyplease`.

## Blocking `reqwest` client

The client returns `Result<<Op>Response, ClientError>`.
The default mode is blocking. It does not force an async runtime.

## Derive HTTP status semantics

Status codes come from `StatusCode` types in `http`, `axum`, and `reqwest`.
The project does not keep a separate manual status table.

## Direction-aware serde derives

Generated models derive only the serde traits they need.
Server-only response models derive `Serialize`.
Server-only request models derive `Deserialize`.
When both directions are needed, models derive both.

This avoids impossible trait bounds on reused `x-rust-type` targets.

## Vendor extensions

Supported keys:

- `x-rust-type`
- `x-rust-name`
- `x-rust-serde-skip`
- Compatibility keys: `x-omitempty`, `x-order`, `x-deprecated-reason`,
  `x-enum-varnames`, `x-enumNames`

`x-go-*` keys are accepted but ignored.

## Combined output at the crate root

Generated models, per-operation types, server interfaces, and client interfaces
are emitted at the crate root in one file.

If a generated per-operation type name conflicts with a component model name,
generation fails. Use `x-rust-name` or `response-type-suffix` to resolve it.

## Type-name collisions fail fast

- **Go:** `oapi-codegen` adds a numeric suffix when two names collapse onto one
  Go identifier. The second type becomes `MyWidget2`.
- **Rust:** two component schemas that produce one Rust type name are an error.
  For example, `order-item` and `orderItem` both produce `OrderItem`.

A numeric suffix picks a public type name for the author. The name carries no
meaning, and the mapping from `MyWidget2` back to a schema is not clear to a
reader of the generated code. Worse, the number depends on document order, so a
later edit to the spec can move `MyWidget2` onto a different schema and change
the meaning of code that already compiles.

The generator therefore stops and names both schemas. The error also gives the
two remedies:

- Put `x-rust-name` on one of the two schemas. This records the name the author
  wants, and it changes that one schema only.
- Set `output-options.type-name-suffix`. The generator then adds that suffix to
  each colliding schema after the first. See
  [configuration](configuration.md#type-name-suffix).

One pass collects every collision in the document, so the report lists all of
them at once. Numbered suffixes remain in use inside an enum, where the variant
names belong to one type and a reader sees them next to their wire values.

A collision is an error only when the generated file holds both schemas. Default
pruning drops a schema that no generated operation reaches, and two dropped
schemas cannot collide in a file that holds neither of them. The check therefore
runs after pruning. With `output-options.skip-prune`, or with models-only
generation, the file holds every schema and every collision reports.

## Method-name collisions fail fast

The same rule covers the name of a generated method. Two `operationId` values that
differ only in case or in punctuation collapse onto one Rust name. For example,
`list-widgets` and `listWidgets` both give `list_widgets`. An operation with no
`operationId` takes its name from the method and the path, and two such paths can
collide as well.

Every artifact of an operation derives from that one name, so a collision emits a
duplicate trait method, response enum, and handler, and the router points both
routes at one handler. The generator stops and names both operations by method and
path.

There is only one remedy, and it is `x-rust-name` on one of the two operations. No
suffix option exists here, unlike `type-name-suffix` for a type name. A consumer
writes each method name into an `impl` block, so every method name stays the choice
of the spec author. See [extensions](extensions.md#operation-names).

Filtering runs before lowering, so an operation that
`output-options.exclude-operation-ids` removes claims no name and joins no
collision. One pass over the paths collects every collision, so the report lists all
of them at once.

## OpenAPI 3.0

The generator reads OpenAPI 3.0 documents through the `openapiv3` crate.

## Config compatibility

YAML config keys mirror `oapi-codegen`.
Unknown keys are ignored.

## Explicit invocation

- **Go:** A bare command can generate output from defaults.
- **Rust:** You must pass `--config-file`. The config must enable at least one
  artifact. You must set output with `--output-file` or config `output:`.

If generation produces no code, the tool fails with guidance.
