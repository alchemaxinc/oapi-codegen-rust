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

## Non-serde derives follow the foreign types a model reaches

Generated models also derive `Debug`, `Clone`, and `PartialEq`. Direction does not
narrow these three. A bound on an `x-rust-type` target is forced
whichever way the data flows. The trait is for the consumer of the model, and not
for serde.

So the specification declares it, with `x-rust-derive` on the target schema. The generator then drops a trait the target lacks from every
model that reaches the target. It drops the same trait from every per-operation type
that holds one of those models. An absent key claims all three traits, so a
specification written before this key emits the same output as before.

The constraint travels up the reference graph, and the direction walk travels down
it. A reference points from a model to the model it holds, and a trait bound points
the other way. Both walks read the same graph.

## Vendor extensions

Supported keys:

- `x-rust-type`
- `x-rust-name`
- `x-rust-serde-skip`
- `x-rust-derive`
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

The generator does not read a newer document as a 3.0 document, because the
dialects overlap. A newer document whose every construct also parses as 3.0 would
generate without a message. One newer construct in that same document then fails
with a message that names a YAML shape and not a version. The gate reports the
version instead.

The gate reads the `openapi:` key before it reads the rest of the document, which
is what makes the second case report correctly.

The gate applies to a referenced file as well. A file that `$ref` reaches is a
document of its own and declares its own version, so a newer fragment inside a
3.0 document is the same overlap as a newer root document.

Support for a newer version is a question of the parser behind the generator, and
this document does not track it. Read the version table in the root
[README](../README.md) for the current state.

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

## A recursive type gets a `Box`

A schema may refer to itself, directly or through other schemas. Written out as
it stands, `Node { child: Node }` is a type that holds itself, and rustc rejects
it with `E0072`. The generator inserts a `Box` on the field that closes the
cycle, which is the fix rustc itself suggests.

A field holds its type when the size of that type counts towards the size of the
struct:

- `Vec<T>` and `HashMap<String, T>` keep their elements on the heap. They hold
  nothing, so a cycle through an array or a map already has a size and gets no
  box.
- `Option<T>` stores its `T` inline. `Option<Node>` inside `Node` is just as
  infinite as `Node`, so it becomes `Option<Box<Node>>`. Making a recursive
  property optional does not fix anything on its own.

Every edge on a cycle gets a box, and not one chosen edge. One box is enough for
rustc, but the choice would fall out of the order the items sit in, so two
schemas that refer to each other would get a box on whichever came first. Boxing
both states the same fact about both.

`Box` is invisible to serde, so the wire format does not change.

The one cycle this cannot fix is a cycle of `$ref`s that declare nothing else.
Each such schema becomes a type alias, and `type A = B; type B = A;` is an error
(`E0391`) that no indirection removes. The generator reports it and names the
schemas on the cycle.

## The server names its security but does not enforce it

A document's `security` says which credential an operation expects. The
generated `Api` trait says so too: a protected method carries a `# Security`
section naming each scheme and where its credential sits, for example a bearer
token in the `Authorization` header or an API key in the `X-API-Key` header. An
operation with `security: []` carries no such note, so a public operation is
easy to tell from a protected one.

The generator stops there. It emits no check, no extractor, and no middleware.
Verifying a credential needs knowledge only the application has: which signing
key, which issuer, which claim names which user, and what a failure should
return. Guessing any of that would produce a check that looks real and is not,
which is worse than none. Read the credential in the trait method and verify it
there.

This also holds for schemes the client refuses, such as OAuth2. A server reads
credentials rather than sending them, so nothing is rejected; the doc comment
names the scheme and leaves the rest to the implementation.

## Config compatibility

YAML config keys mirror `oapi-codegen`.
Unknown keys are ignored.

## Explicit invocation

- **Go:** A bare command can generate output from defaults.
- **Rust:** You must pass `--config-file`. The config must enable at least one
  artifact. You must set output with `--output-file` or config `output:`.

If generation produces no code, the tool fails with guidance.
