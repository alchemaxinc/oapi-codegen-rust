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
  std-http-server: true # also emit an axum server interface
```

`import-mapping` maps a referenced spec file to the Rust module its schemas are
emitted into, so cross-file `$ref`s in the server interface resolve to
`that_module::TypeName` (see the server section below). Other unknown keys (e.g.
`output-options`) are accepted and ignored so existing `oapi-codegen` configs can
be reused.

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

## Server generation

Setting `generate.std-http-server` emits an [`axum`] interface alongside the
models. The output is typed-only — the generator never decides how a request is
handled, it only describes the contract:

- a `trait Api` with one method per operation returning an
  `impl Future<Output = ...> + Send` (native async-in-traits / RPITIT, no
  `async-trait` dependency), whose arguments are the path parameters and the
  decoded JSON body;
- a response `enum` per operation, with one variant per documented status code,
  implementing `axum::response::IntoResponse`;
- a `router<T: Api>(api: T) -> axum::Router` builder that wires each operation to
  its route, reusing the OpenAPI path template verbatim (axum 0.8 uses the same
  `/{id}` syntax).

You implement `Api` for your own type and pass it to `router`; the generated
code owns extraction, status codes, and JSON (de)serialization.

A complete, runnable demonstration lives in [`examples/bookstore`](examples/bookstore):
a multi-file spec (`schemas/common.yaml`, `schemas/catalog.yaml`, `openapi.yaml`)
generates one model module per schema file plus an axum server whose cross-file
`$ref`s resolve to those modules via `import-mapping`. The whole thing compiles
as a single crate, and `tests/smoke.rs` builds a `router` from a hand-written
`Api` impl. Regenerate it with `make generate-example`.

This is a deliberate first slice. Currently supported: path parameters (inline
scalars, or a same-document `$ref` that resolves to a scalar), JSON request
bodies (a `$ref` or a scalar), and responses keyed by explicit status codes —
including component `$ref` responses (`#/components/responses/...`) resolved
against the document. Cross-file schema `$ref`s in bodies and responses are
routed through `import-mapping` to an external module type (e.g.
`crate::apimodel::Widget`). Query, header, and cookie parameters are **ignored**.
A `default` or range (`5XX`) response, a component-level `$ref` _parameter_ or
_request body_, a cross-file component-_response_ `$ref`, or a non-scalar path
parameter is **rejected** with an error rather than mis-generated.

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
- **Partly supported** (the axum server generator, see above): `paths`,
  `parameters` (path only), `requestBody` (JSON), and `responses` (explicit
  status codes).
- **Planned** (the remaining server/client surface): `securitySchemes`,
  `servers`, `callbacks`, and `links`.

[`axum`]: https://crates.io/crates/axum
[`openapiv3`]: https://crates.io/crates/openapiv3
[`quote`]: https://crates.io/crates/quote
[`syn`]: https://crates.io/crates/syn
[`prettyplease`]: https://crates.io/crates/prettyplease
