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

## One run reports every independent problem

A run does not stop at the first fault it finds. Component schemas are independent
of each other, so a fault in one says nothing about the next. The generator lowers
them all and reports the faults together.

```text
error: found 3 problems in the spec.
  1. `x-order` on `Alpha.id` needs a whole number, but the document gives a string
  2. `x-order` on `Beta.id` needs a whole number, but the document gives a string
  3. `x-order` on `Gamma.id` needs a whole number, but the document gives a string
```

Without this, a document with three faults costs three runs, because each run shows
one fault and hides the rest.

Two rules keep the report short and true.

A check whose result the rest of the run needs still stops at once. An unresolved
`$ref` is one example. A run that continues past it reports later faults that are
only effects of the first one, and the author reads a list that is mostly noise.

One fault is reported as itself. The count and the list appear for two or more.

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
and not on an inline schema. A member of a `oneOf` list is the one exception,
because that member becomes a variant that needs a name of its own. See
[Union variants](extensions.md#union-variants).

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

## A prelude name is not free

The section above is about two items that take one name. A model named `Option`
is a different failure. It emits one item, duplicates nothing, and passes every
collision check. It hides the prelude `Option` for the whole file instead, so
each `Option<String>` there reads as that struct and takes no argument. The file
stops compiling.

Five names carry this risk, because the emitted file writes them without a path:
`Option`, `String`, `Vec`, `Box`, and `Result`. A model that takes one of them
fails generation, and the remedy is `x-rust-name` or
`output-options.type-name-suffix`.

`Result` is held only when a server or a client is generated. Models alone name
no `Result`, so a models-only run accepts a schema of that name. This matches the
rule for `Api` and `Client`: a name is held only when the run writes it.

Within a target the check reads names, not uses, so it is wider than it has to
be. A spec with a schema named `Box` and no recursion writes no `Box<T>`, and it
would compile, but it is rejected all the same. The trade is deliberate. A false
rejection is loud and the hint gives the remedy on one line, while a missed use
site emits code that does not compile. The verdict also stays put: adding a
recursive schema later cannot turn an accepted name into a broken build.

`Ok`, `Err`, `Some`, and `None` are free, and belong on no such list, but the
reason is narrow enough to write down. Those name values. A **braced** `struct`,
an `enum`, and an alias each take a type name only, so `Ok(..)` in generated code
still finds the prelude. A **tuple** or **unit** `struct` would take the value
name as well, and a model named `Ok` would then hide the prelude variant.

The generator writes `pub struct Name {..}` at every site, so the rule holds. It
is an invariant rather than an accident, and `every_generated_struct_is_braced`
states it. The `combined_prelude_value_names` fixture compiles the adversarial
case: an operation references each of the four names, so none is pruned, and a
server and a client then write all four without a path beside them. The generated
file holds `Ok(Ok)`, `Ok(Some)`, and a `NotFound(None)` variant a few lines above
a plain prelude `None`.

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

## An integer `enum` becomes a real enum

`enum` on an `integer` schema gives a Rust `enum`, and not a bare `i64`. Each
value becomes a unit variant with the value as its discriminant.

```yaml
Priority:
  type: integer
  enum: [1, 2, 3]
```

```rust
#[serde(try_from = "i64", into = "i64")]
#[repr(i64)]
pub enum Priority {
    Value1 = 1,
    Value2 = 2,
    Value3 = 3,
}
```

The `repr` follows the `format`, so `int32` gives `i32`. A generated
`From` and `TryFrom` pair carries the value, and the wire form stays a bare
number. A number the document does not list fails to deserialize, and the message
names the type and the value.

Each `serde` key drives one trait, so the generator writes `try_from` only with
`Deserialize` and `into` only with `Serialize`. A response-only model therefore
gets `into` alone.

A default variant name comes from the value: `1` gives `Value1`, and `-1` gives
`ValueMinus1`. These names are poor, so give `x-enum-varnames` as a string enum
does.

An `enum` names each value once. A repeat is an error, for a string enum and an
integer enum alike. Two integer variants cannot share one discriminant
(`E0081`), and two string variants that share one `rename` leave the second
unreachable. An integer value the `repr` cannot hold is an error for the same
reason: the literal does not fit.

A `number` or `boolean` `enum` still lowers to a bare `f64` or `bool`. A float is
not a legal discriminant, and a boolean enum names nothing useful.

## A union cannot hold one type twice

A `oneOf` or an `anyOf` becomes an enum with `#[serde(untagged)]`, so no tag
appears on the wire and serde picks a variant by shape. It reads the variants in
declaration order and takes the first that fits.

That order makes a repeated type unreachable. A union whose members give `Cat`
twice compiles, and the second variant never matches: a value built with it
serializes like the first and reads back as the first. The value changes, and
nothing reports it. So a repeated type is an error, and the message names the two
variants that share it.

The check reads the lowered Rust type, not the schema. Two members that differ in
the document but reach one type still collide, which is the case that matters,
because the wire is all serde sees.

The check does not read shapes. Two distinct types with the same fields still
shadow each other at run time, and the generator accepts them. Deciding that in
general means comparing every optional field and every subset, so the generator
draws the line at a repeated type, where the fault is exact.

A variant takes its name from `x-rust-name`, else from the type a `$ref` names,
else from the type an inline member holds when it hoists none of its own. An
inline member that hoists a type gets no name from the generator, and generation
stops until the author gives one. A position would name it, but a position
carries no meaning and it moves: swapping two members would point one name at the
other shape and change what already-compiling code means. This is the argument
that [type-name collisions](#type-name-collisions-fail-fast) already make.

## A `minimum` of zero or more gives an unsigned type

OpenAPI has no unsigned format. `format` gives the width of an integer, and
`int32` and `int64` are both signed. So a document that holds a count, a
percentage, or an identifier has one place to say the value is never negative:
`minimum`.

The generator reads it. `format: int32` with a `minimum` of zero or more gives
`u32`, `int64` gives `u64`, and an integer with no `format` gives `u64` where it
gave `i64`. A negative `minimum`, or no `minimum` at all, leaves the type signed.
Any bound at zero or above says the same thing about the sign, so `minimum: 5`
gives an unsigned type too. `exclusiveMinimum` counts as well: a whole number
above `-1` is zero or more, so the generator folds the flag into the bound
beside it and reads the result.

The type then states the rule, and a reader needs no check to know it. This also
carries a round trip: a code-first generator writes `minimum: 0` because the Rust
type was unsigned, so reading it back returns the type the author started with.

The bound stays a bound. It still becomes a check where the value comes in,
except where the type already refuses every value the check would reject: a
`minimum` of zero on an unsigned type writes nothing, because no value fails it.
This is the rule `minLength: 0` already follows. The same holds at the other
end, so a `maximum` of 2147483647 on `i32` writes nothing. A bound with values
left to reject still writes its check.

A value below zero is still rejected, by serde rather than by a generated check,
so the message names the type instead of the bound.

Folding a flag into the bound beside it can carry that bound past the end of the
type. `maximum: -2147483648` with `exclusiveMaximum` on `int32` asks for a value
below where `i32` starts, and no value answers. The generator refuses the
document, because the code it would write refuses every request. A `minimum`
above a `maximum` reads the same way and gets the same answer, as does a pair of
bounds that meet on one value an `exclusive` flag then leaves out. These two last
readings need no type, so a `number` takes them as an `integer` does. A bound the
author writes out of range is a separate fault, and it keeps the message about
width.

An integer `enum` takes its `repr` the same way, so `format: int32` with a
`minimum` of zero gives `#[repr(u32)]`. A negative value in that `enum` and a
`minimum` of zero disagree, and no Rust type holds both, so the generator names
the fault instead of writing a file that does not build.

A `default` follows the type. An unsigned field reads its `default` as unsigned,
which refuses a negative value and reaches the whole range of `u64`, above where
`i64` stops.

## Value constraints are checked when the value comes in

A schema can narrow the values it accepts with `pattern`, `minimum`, `minItems`,
and ten more keywords. The generator writes these as checks, and serde runs them
while it reads the value. A bad value fails to parse, so it never reaches a
handler.

The check runs on the way in only. A model that carries a response gets no check,
because the code that builds the response owns the value. This is the
same rule the derives follow: a direction the output does not use costs nothing.

A field with a check takes `#[serde(deserialize_with = "...")]`, and the struct
takes the function beside its defaults. `deserialize_with` makes serde read the
field even when the payload omits it, so an optional field without a `default`
also takes a bare `#[serde(default)]`. Without it, an absent optional field
fails to parse.

A length counts characters, not bytes, as JSON Schema states. The check reads one
character past the bound and stops there, so a long string from an untrusted
caller costs the bound and not its own length. A `minLength` of zero writes no
check, because no string fails it.

`uniqueItems` applies to a list of scalars only, because a model that reaches a
foreign type can lose `PartialEq`. A hashable element goes into a set, which
reads the list one time. A `f64` has neither `Eq` nor `Hash`, so a list of them
compares each pair instead. That costs the square of the length, and a server
takes the list from an untrusted caller, so write `maxItems` beside
`uniqueItems`.

`minProperties` and `maxProperties` reach a map only. A struct declares its
properties, so the count is already fixed when the file compiles.

A `pattern` becomes a `regex::Regex`, built one time and held. OpenAPI writes a
pattern in ECMA-262, which has look-ahead and back-references. Rust regular
expressions have neither. So the generator reads every pattern at generation
time and stops on one it cannot hold, rather than write a file that does not
build. This is also why the `expect` beside the built regex cannot fire.

A `$ref` to a constrained scalar makes a type alias, and an alias carries no
serde attribute. The field that names the alias takes the checks instead. A
target with an `enum` or an `x-rust-type` is left alone: there the name and the
type below it are not the same thing.

A rule that cannot reach its type is an error, not a silence. A `format` of
`date`, `date-time`, `uuid`, or `binary` names a type that is no longer a string,
so a `pattern` there reads nothing once the value is parsed. An `x-rust-type`
does the same. `minProperties` on a schema that names its properties, and
`uniqueItems` on a list of models, cannot run either. Each stops the run and
names the way out, because a rule the document states and the code drops is worse
than no rule at all.

`readOnly` and `writeOnly` are read by nothing yet. They mark a direction, not a
value, and one struct cannot be both.

## A `default` removes the `Option`

An optional property with a `default` lowers to a plain `T`, not `Option<T>`. A
`#[serde(default = "..")]` attribute points at an associated function:

```rust
pub struct Widget {
    #[serde(default = "Widget::default_count")]
    pub count: i64,
}

impl Widget {
    fn default_count() -> i64 {
        10
    }
}
```

The document says the value is `10` when the key is absent. So the field always
holds a value after a parse, and an `Option` there only ever holds `Some`.

The function is associated, not free. This keeps it out of the crate root, where
every generated type lives. Field names are unique within a struct, so these
names are unique too.

`nullable` is the exception. There `null` is a value that the property carries.
The `Option` stays, and the default fills its `Some` side. A `default: null` does
nothing. The parser reads it as no default at all, and serde already leaves a
missing `Option` as `None`.

The generator ignores a `default` on a **required** property. The property is
always present. Use the default, and a payload that omits a required property
becomes valid.

Only a value with a literal form works: a string, a number, a boolean, an enum
value, an empty array, and an empty object. "An empty object" means a free-form
map, from `additionalProperties`. A `default: {}` on a property with named
properties is an error, because a struct has no such literal. A non-empty array,
a non-empty object, and a value of the wrong type are errors too, not silent
drops. A dropped default leaves the document and the code in disagreement. An
`int32` property with a default outside the range of `i32` is an error for the
same reason. The alternative is generated code that does not compile.

A query parameter follows the same rule. A `default: 20` on `limit` gives a
plain `i32` field. The default applies when the request omits `limit`. An empty
`?limit=` is not an omission. It is the text `""`, and a parse of it into an
`i32` fails.

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
which is worse than none.

Enforce it outside the generated code, in a `tower` layer around the router. A
trait method takes only the parameters the operation declares, so the credential
never reaches it: a bearer token or an API key is not an OpenAPI parameter. Pass
whatever the layer works out, such as a user identity, through the state your
`Api` implementation already holds, or through a request extension the layer
inserts.

The listed schemes are a union, not a rule. A document can require two schemes at
once, or offer a choice of several, and the generated list flattens both into the
same set of names. Past one name, read the document's `security` for the exact
rule.

This also holds for schemes the client refuses, such as OAuth2. A server reads
credentials rather than sending them, so nothing is rejected; the doc comment
names the scheme and leaves the rest to the implementation.

## Config compatibility

YAML config keys mirror `oapi-codegen`.
Unknown keys are ignored.

## Output types are `non_exhaustive`, input types are not

`#[non_exhaustive]` stops a struct expression outside the crate altogether, even
one with `..Default::default()`. On an enum it costs much less: variant
construction stays open, and only an exhaustive `match` must add a catch-all
arm. The two cases therefore get different answers.

The IR enums and `Error` carry the attribute. A caller reads these and matches
on them, so a new IR node or a new error must not break that caller's build.

The IR structs do not carry it, and neither do `Config`, `Generate`,
`OutputOptions` and `Targets`. A caller builds all of these: the config types
name a run, and `emit_module` takes an IR, so a hand-built `Module` is a
supported input. The attribute would leave these functions with no reachable
input at all. They derive `Default` instead, so `..Default::default()` absorbs a
new field. `tests/api_stability.rs` holds both halves of this rule.

The attribute has no effect inside the crate, so the generator's own matches on
`Error` and on the IR stay exhaustive and still fail to compile when a variant
arrives. Only the `hints_for` match in the binary takes a catch-all, because the
binary is a separate crate.

## Explicit invocation

- **Go:** A bare command can generate output from defaults.
- **Rust:** You must pass `--config-file`. The config must enable at least one
  artifact. You must set output with `--output-file` or config `output:`.

If generation produces no code, the tool fails with guidance.
