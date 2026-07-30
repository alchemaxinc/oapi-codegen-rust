# OpenAPI extensions

The generator supports the vendor extensions below.
Use `x-rust-*` keys for Rust-specific behavior.
The generator ignores other keys, including `x-go-type`, `x-go-name`, and
`x-go-json-ignore`.

| Extension                         | Applies to                             | Effect                                                                         |
| --------------------------------- | -------------------------------------- | ------------------------------------------------------------------------------ |
| `x-rust-type`                     | schema                                 | Emit this Rust type as-is instead of a generated type.                         |
| `x-rust-name`                     | schema / property / operation / server | Override the generated type name, field name, method name, or server URL name. |
| `x-rust-serde-skip`               | property                               | Omit the field with `#[serde(skip)]`.                                          |
| `x-omitempty`                     | property                               | Force `skip_serializing_if` on or off.                                         |
| `x-order`                         | property                               | Set explicit field order (1-indexed).                                          |
| `x-deprecated-reason`             | schema / property                      | Note for `#[deprecated]`. Used only when `deprecated: true`.                   |
| `x-enum-varnames` / `x-enumNames` | enum schema                            | Override generated enum variant names by position.                             |

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
an inline schema. Go's `oapi-codegen` documents `x-go-name` the same way.

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
