# OpenAPI extensions

The generator supports the vendor extensions below.
Use `x-rust-*` keys for Rust-specific behavior.
The generator ignores other keys, including `x-go-type`, `x-go-name`, and
`x-go-json-ignore`.

| Extension                         | Applies to                             | Effect                                                                         |
| --------------------------------- | -------------------------------------- | ------------------------------------------------------------------------------ |
| `x-rust-type`                     | schema                                 | Emit this Rust type as-is instead of a generated type.                         |
| `x-rust-derive`                   | schema with `x-rust-type`              | Which of `Debug`, `Clone`, `PartialEq` the target type implements.             |
| `x-rust-name`                     | schema / property / operation / server | Override the generated type name, field name, method name, or server URL name. |
| `x-rust-serde-skip`               | property                               | Omit the field with `#[serde(skip)]`.                                          |
| `x-omitempty`                     | property                               | Force `skip_serializing_if` on or off.                                         |
| `x-order`                         | property                               | Set explicit field order (1-indexed).                                          |
| `x-deprecated-reason`             | schema / property                      | Note for `#[deprecated]`. Used only when `deprecated: true`.                   |
| `x-enum-varnames` / `x-enumNames` | enum schema                            | Override generated enum variant names by position.                             |

## The kind of value a key needs

| Extension                         | Value                 |
| --------------------------------- | --------------------- |
| `x-rust-type`                     | a string              |
| `x-rust-name`                     | a string              |
| `x-deprecated-reason`             | a string              |
| `x-rust-serde-skip`               | `true` or `false`     |
| `x-omitempty`                     | `true` or `false`     |
| `x-order`                         | a whole number        |
| `x-rust-derive`                   | a list of trait names |
| `x-enum-varnames` / `x-enumNames` | a list of strings     |

A key that carries a value of a different kind ends the run. The generator does
not fall back to the default, because the author wrote the key to change
something. A fallback would read the same as an absent key, so a typo such as
`x-order: "first"` would stay hidden.

## Example

```yaml
components:
  schemas:
    Widget:
      type: object
      properties:
        id:
          type: string
          x-rust-name: widget_id
        cached_at:
          type: string
          x-rust-serde-skip: true
        raw:
          x-rust-type: serde_json::Value
```

## Traits of an `x-rust-type` target

Every generated model derives `Debug`, `Clone`, and `PartialEq`. A model that
holds a type the generator did not write cannot derive a trait that type lacks.
The generator cannot look at the type either, because the specification names it
as text and `rustc` resolves it much later. So `x-rust-derive` states it.

List the traits the target does implement:

```yaml
components:
  schemas:
    Opaque:
      type: string
      x-rust-type: crate::domain::Opaque
      x-rust-derive: [Debug]
```

The generator then drops `Clone` and `PartialEq` from every model that reaches
`Opaque`, and from every per-operation type that holds one of those models. A
dropped trait costs one trait on those types. An emitted trait the target cannot
satisfy costs a build.

An absent `x-rust-derive` claims all three traits. That keeps the output of every
specification written before this key identical, and it is right most of the
time. `uuid::Uuid`, the `chrono` types, and a typical hand-written domain type
all derive the three. An empty list claims none of them.

The key accepts only `Debug`, `Clone`, and `PartialEq`. Any other name is an
error. A misspelled name that the generator ignored would claim nothing and give
the build error the key exists to prevent.

The key belongs beside `x-rust-type` on the same schema. It has no effect
anywhere else.

## Server URL names

A `servers:` entry takes its name from `x-rust-name` first, then from
`description`, and last from the URL. The `server url` prefix applies to a name
that comes from a description or a URL, so a description of `Production` gives
`SERVER_URL_PRODUCTION`. An `x-rust-name` replaces the whole seed and takes no
prefix. `generate.server-urls` must be on for any of this to emit.

```yaml
servers:
  - url: https://api.example.com
    description: Production
    x-rust-name: prod
```

## Operation names

The name of a generated method comes from `x-rust-name` first, then from
`operationId`, and last from the method and the path together. For example, `get`
on `/v1/widgets` gives `get_v1_widgets`.

Every artifact of an operation derives from that one name. This includes the trait
method, the parameter structs, the response enum, and the axum handler. Two
operations that produce one name therefore produce duplicate items, and the
generated file does not compile. The generator reports the clash and stops.

Two `operationId` values that differ only in case or in punctuation cause this.
`list-widgets` and `listWidgets` both give `list_widgets`. To resolve the clash,
change one `operationId`, or put `x-rust-name` on one of the two operations:

```yaml
paths:
  /widgets:
    get:
      operationId: list-widgets
      responses:
        "200":
          description: ok
  /gadgets:
    get:
      operationId: listWidgets
      x-rust-name: list_gadgets
      responses:
        "200":
          description: ok
```

There is no suffix option for a method name, unlike `type-name-suffix` for a type
name. A consumer of the generated code writes each method name into an `impl`
block, so every method name stays the choice of the spec author.

## Inline schemas

`x-rust-name` applies to a top-level schema and to a property. It does not apply to
an inline schema, with one exception: a member of a `oneOf` or an `anyOf` list.
Go's `oapi-codegen` documents `x-go-name` the same way for the general case.

A member of a `oneOf` list becomes a variant of the generated enum. That variant
needs a name, and an inline object gives none, so generation stops until the
author gives one. `x-rust-name` on the member gives it, and it names the hoisted
type too, so the two agree:

```yaml
components:
  schemas:
    Pet:
      oneOf:
        - x-rust-name: Cat
          type: object
          required: [meow]
          properties:
            meow:
              type: string
```

This gives `Pet::Cat(PetCat)`. Without it the member has no name and generation
stops. See [Union variants](#union-variants) for the members that do carry a name
of their own, and for the name the hoisted type takes.

The generator hoists an inline object to the crate root and names it after the
property path that encloses it. The inline `bar` property of schema `Foo` therefore
gives an item named `FooBar`. A component schema named `FooBar` gives that same
name, so the file holds two items with one name and does not compile. The generator
reports the clash and stops.

Two remedies apply. Put `x-rust-name` on the component schema that encloses the
inline object, or move the inline object into a component schema of its own and
refer to it with `$ref`:

```yaml
components:
  schemas:
    Foo:
      type: object
      properties:
        bar:
          $ref: "#/components/schemas/FooBarInner"
    FooBarInner:
      type: object
      properties:
        x:
          type: string
    FooBar:
      type: object
      properties:
        sku:
          type: string
```

## Union variants

A `oneOf` or an `anyOf` becomes an untagged enum. Each member becomes a variant,
and the name of that variant comes from the first rule below that applies.

| Member                                          | Variant name                                                                                  |
| ----------------------------------------------- | --------------------------------------------------------------------------------------------- |
| Carries `x-rust-name`                           | The name it gives                                                                             |
| A `$ref`                                        | The name of the type it points to                                                             |
| A string `enum` of one value                    | That value                                                                                    |
| An inline member that hoists no type of its own | The type it holds: `String`, `I32`, `I64`, `F64`, `Bool`, `Date`, `DateTime`, `Uuid`, `Bytes` |
| Any other inline member                         | None. Generation stops.                                                                       |

A member that hoists no type is named by the type it holds. A union cannot hold
one type twice, so these names stay unique.

A member that holds one `enum` value stands for a constant, and that value says
what the member is. A document that writes a Rust enum as a `oneOf` gives every
unit variant this shape, so a list of constants needs no `x-rust-name`:

```yaml
Signal:
  oneOf:
    - { type: string, enum: [red] }
    - { type: string, enum: [amber] }
```

This gives `Signal::Red(SignalRed)` and `Signal::Amber(SignalAmber)`. The next
section says where those payload names come from.

Every other inline member must carry `x-rust-name`, or move into a component
schema that a `$ref` points at. An object, a list, and a map each hoist a type
that needs a name of its own, and only the author can give one that means
anything. The position could name them, but the generator does not use it: a
position carries no meaning, and it moves. Swapping two members of a `oneOf` would
point `Variant0` at the other shape and change what already-compiling code means.
This follows the rule that
[type-name collisions](design.md#type-name-collisions-fail-fast) already use.

### The name of a hoisted member type

A member that hoists puts its type at the crate root, and the union name goes in
front of it. Member `Red` of union `Signal` therefore gives `SignalRed`. This is
the rule an inline property already uses, where `Foo.bar` gives `FooBar`.

The variant itself keeps the short name, because the enum in front of it already
says which union it belongs to. `Signal::Red(SignalRed)` reads once at the use
site and stays unique at the crate root. Two unions that both hold a member named
`Unknown` would otherwise take one name and stop generation.

Two members that lower to one type also end generation. Serde reads an untagged
enum in order and takes the first variant that fits, so the second one never
matches. A value built with it comes back as the first variant, which changes the
value and reports nothing. Remove the repeated member.
