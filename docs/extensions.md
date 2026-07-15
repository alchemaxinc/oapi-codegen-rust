# OpenAPI extensions

The generator honors the vendor extensions below. Rust-specific behaviour uses
`x-rust-*` keys; the rest are `oapi-codegen` compatibility keys. Any other
extension — including the Go type keys `x-go-type`, `x-go-name`,
`x-go-json-ignore` — is ignored (accepted, but has no effect); use the
`x-rust-*` equivalents instead.

| Extension                         | Applies to        | Effect                                                          |
| --------------------------------- | ----------------- | --------------------------------------------------------------- |
| `x-rust-type`                     | schema            | Emit this verbatim Rust type instead of a generated one.        |
| `x-rust-name`                     | schema / property | Override the generated type or field identifier.                |
| `x-rust-serde-skip`               | property          | Drop the field with `#[serde(skip)]`.                           |
| `x-omitempty`                     | property          | Force `skip_serializing_if` on/off, overriding the default.     |
| `x-order`                         | property          | Order struct fields explicitly (1-indexed).                     |
| `x-deprecated-reason`             | schema / property | Note for `#[deprecated]`; honored only when `deprecated: true`. |
| `x-enum-varnames` / `x-enumNames` | enum schema       | Override generated enum variant identifiers, positionally.      |

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
