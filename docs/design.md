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

## One namespace holds every generated type

The flat layout puts every generated type at the crate root. Four kinds of type
share one namespace there. These are the component models, the inline schemas that
the generator hoists, the per-operation types, and the generator interfaces (`Api`,
`Client`, and `ClientError`). Any two of them that take one name emit two items with
that name, which does not compile. One check therefore holds one namespace and
reports every name that two items take. Three cases reach it.

A hoisted inline schema takes its name from the property path that encloses it. The
inline `bar` property of schema `Foo` gives `FooBar`, which is also the name that a
component schema `FooBar` gives. Name resolution compares `components` entries only,
so it cannot see this pair. The check reads the final item names instead.

An inline schema carries no name of its own, so `x-rust-name` on that schema has
nothing to override. The remedy acts on the component schema that encloses it, or it
moves the inline schema into a component of its own. This matches Go's
`oapi-codegen`, which documents `x-go-name` on a component schema and on a property,
and not on an inline schema.

A per-operation type can take the name of a model. A schema named `<Op>Response` is
the common case.

A per-operation type can take the name of a generator interface. An operation named
`api` gives a response enum named `Api`, which is also the name of the server
interface trait. The interface name is fixed, so only the operation side can move. A
reserved name applies only when its target is requested, so a client-only run does
not reserve `Api`.

A per-operation type can also take the name of another per-operation type. Every
such name is a method name plus a fixed suffix, so a `response-type-suffix` such as
`Query` makes one operation's response enum take the name of a query-parameter
struct. Two types of one operation need a different suffix, because no method name
separates them. Two types of different operations take `x-rust-name` on one of the
two operations.

## OpenAPI 3.0 only, and the version gate

The generator reads OpenAPI 3.0 documents through the `openapiv3` crate. Every
document must declare a `3.0.x` version in its `openapi:` key. A document that
declares any other version is an error.

The generator does not read a 3.1 document as a 3.0 document, because the two
subsets overlap. A 3.1 document whose every construct also parses as 3.0 would
generate without a message. One 3.1-only construct in that same document then
fails with a `serde` message that names neither the version nor the reason. The
gate reports the version instead.

The gate reads the `openapi:` key before it reads the rest of the document, so a
3.1-only construct reports as a version and not as a YAML shape.

The gate applies to a referenced file as well. A file that `$ref` reaches is a
document of its own and declares its own version. A 3.1 fragment inside a 3.0
document is the same overlap as a 3.1 root document.

A document must also declare no `webhooks:` key. That key carries operations,
and the generator emits no handler for them. Silence about the key reads as "the
document declares no such operation", so the generator rejects the key.

## A body must declare a content type the generator can represent

A request body must declare one of `application/json`,
`application/x-www-form-urlencoded`, `multipart/form-data`, or `text/plain`. A
response body must declare one of the first three of those. Multipart is absent
from the response list because `axum` has a multipart extractor and no multipart
response writer.

A body that declares `content:`, and no content type from its list, is an error.
Such a body is not a bodyless body. A bodyless response declares no `content:`
at all, and `204` is the common case. A response that declares
`application/pdf` states that a payload exists, so a bodyless variant for it
drops the payload without a message.

## Unknown fields follow `additionalProperties`

- `additionalProperties: false` gives `#[serde(deny_unknown_fields)]` on the
  struct. An unknown key in the input is an error.
- An absent `additionalProperties` key gives no attribute. An unknown key in the
  input is dropped, which is what serde does by default.
- An `additionalProperties` schema gives a flattened map field, which holds every
  unknown key.

The generator emits `deny_unknown_fields` only when the struct also derives
`Deserialize`. The `Serialize` derive does not read the attribute.

Two exceptions apply. A merge of `allOf` members drops the key. In JSON Schema
each member validates the whole object, so a member with
`additionalProperties: false` rejects every property that a sibling member
declares. A merge that honored the key would deny the fields that the merge
just added.

A query-parameter struct also drops the key. A query string commonly carries a
parameter that the document does not declare, such as one that a proxy or an
analytics tool adds. A denial there rejects a whole request that the document
permits.

## Config compatibility

YAML config keys mirror `oapi-codegen`.
Unknown keys are ignored.

## Explicit invocation

- **Go:** A bare command can generate output from defaults.
- **Rust:** You must pass `--config-file`. The config must enable at least one
  artifact. You must set output with `--output-file` or config `output:`.

If generation produces no code, the tool fails with guidance.
