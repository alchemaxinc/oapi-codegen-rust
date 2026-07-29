# OpenAPI extensions

The generator supports the vendor extensions below.
Use `x-rust-*` keys for Rust-specific behavior.
The generator ignores other keys, including `x-go-type`, `x-go-name`, and
`x-go-json-ignore`.

| Extension                         | Applies to        | Effect                                                       |
| --------------------------------- | ----------------- | ------------------------------------------------------------ |
| `x-rust-type`                     | schema            | Emit this Rust type as-is instead of a generated type.       |
| `x-rust-name`                     | schema / property | Override the generated type name or field name.              |
| `x-rust-serde-skip`               | property          | Omit the field with `#[serde(skip)]`.                        |
| `x-omitempty`                     | property          | Force `skip_serializing_if` on or off.                       |
| `x-order`                         | property          | Set explicit field order (1-indexed).                        |
| `x-deprecated-reason`             | schema / property | Note for `#[deprecated]`. Used only when `deprecated: true`. |
| `x-enum-varnames` / `x-enumNames` | enum schema       | Override generated enum variant names by position.           |

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
