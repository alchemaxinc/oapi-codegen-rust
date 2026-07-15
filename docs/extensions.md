# OpenAPI extensions

Rust-specific vendor extensions use `x-rust-*` keys. The Go-named equivalents
(`x-go-type`, `x-go-name`, `x-go-json-ignore`) are **not** accepted.

| Extension           | Applies to        | Effect                                                   |
| ------------------- | ----------------- | -------------------------------------------------------- |
| `x-rust-type`       | schema            | Emit this verbatim Rust type instead of a generated one. |
| `x-rust-name`       | schema / property | Override the generated type or field identifier.         |
| `x-rust-serde-skip` | property          | Drop the field with `#[serde(skip)]`.                    |

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
