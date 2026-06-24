# oapi-codegen-rust

Generate idiomatic Rust from OpenAPI 3 specifications, inspired by
[`oapi-codegen`](https://github.com/oapi-codegen/oapi-codegen).

## Usage

```sh
# Print generated models to stdout
oapi-codegen path/to/spec.yaml

# Write to a file
oapi-codegen path/to/spec.yaml -o src/models.rs

# Drive output from an oapi-codegen-style YAML config
oapi-codegen path/to/spec.yaml --config oapi-codegen.yaml
```

The config file mirrors `oapi-codegen`'s format; the relevant keys are:

```yaml
package: apimodel # informational
output: models.rs # output path (overridden by -o)
generate:
  models: true
```

Unknown keys (e.g. `output-options`, `import-mapping`) are accepted and ignored
so existing `oapi-codegen` configs can be reused.

## How it works

A spec is parsed with [`openapiv3`], lowered into an internal representation,
and emitted as a `proc_macro2::TokenStream` via [`quote`], which is parsed into
a [`syn`] AST and pretty-printed with [`prettyplease`]. Building the AST rather
than rendering text templates keeps the output valid-by-construction.

Generated types derive `Serialize`, `Deserialize`, `Debug`, `Clone`, and
`PartialEq`. OpenAPI idioms are mapped to their idiomatic Rust equivalents:

| OpenAPI                              | Rust                                         |
| ------------------------------------ | -------------------------------------------- |
| `object` with properties             | `struct`                                     |
| required vs. optional property       | `T` vs. `Option<T>` + `skip_serializing_if`  |
| inline nested `object`               | hoisted `struct` named `{Parent}{Property}`  |
| `string` `enum`                      | C-like `enum` with `#[serde(rename)]`        |
| `oneOf` / `anyOf`                    | `#[serde(untagged)]` `enum`                  |
| `oneOf` + `discriminator`            | `#[serde(untagged)]` `enum` from the mapping |
| `allOf`                              | one flattened `struct`                       |
| `additionalProperties: {schema}`     | `type Alias = HashMap<String, T>`            |
| `additionalProperties: true`         | `HashMap<String, serde_json::Value>`         |
| `additionalProperties` beside fields | `#[serde(flatten)]` `HashMap` field          |
| empty / free-form schema (`{}`)      | `serde_json::Value`                          |
| local `$ref`                         | the referenced named type (or a type alias)  |
| `nullable`                           | `Option<T>` (even when `required`)           |
| `description`                        | `///` doc comment                            |
| `x-rust-type`                        | verbatim type override                       |

Scalar `format`s map to dedicated types; unknown formats fall back to the base
type:

| `type` + `format`                    | Rust                            |
| ------------------------------------ | ------------------------------- |
| `string` (none / `password`)         | `String`                        |
| `string` / `date`                    | `chrono::NaiveDate`             |
| `string` / `date-time`               | `chrono::DateTime<chrono::Utc>` |
| `string` / `byte` or `binary`        | `Vec<u8>`                       |
| `string` / `uuid`                    | `uuid::Uuid`                    |
| `integer` / `int32`                  | `i32`                           |
| `integer` (none / `int64`)           | `i64`                           |
| `number` (none / `float` / `double`) | `f64`                           |

## Coverage

Every OpenAPI 3 schema element is deliberately catalogued — supported, ignored,
unsupported, or planned — and a coverage matrix test
(`crates/oapi-codegen/tests/coverage.rs`) fails to compile if `openapiv3` ever
grows a schema variant that isn't accounted for, so nothing is silently left in
the unknown.

- **Ignored** (parsed, but no effect on output yet): `title`, `default`,
  `deprecated`, `readOnly`, `writeOnly`, `example`, `externalDocs`,
  `additionalProperties: false`, and `x-go-*` extensions.
- **Unsupported** (rejected with an error rather than mis-generated): `not`.
- **Planned** (the server/client generators): `paths`, `parameters`,
  `requestBody`, `responses`, `securitySchemes`, `servers`, `callbacks`, and
  `links`.

[`openapiv3`]: https://crates.io/crates/openapiv3
[`quote`]: https://crates.io/crates/quote
[`syn`]: https://crates.io/crates/syn
[`prettyplease`]: https://crates.io/crates/prettyplease
