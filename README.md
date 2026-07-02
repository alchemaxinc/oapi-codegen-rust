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
  `async-trait` dependency), whose arguments are the path parameters, a generated
  query-parameter struct (when the operation declares query parameters), a
  generated header-parameter struct (when it declares header parameters), and the
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

This is a deliberate first slice; the generator rejects anything it cannot model
faithfully rather than emit subtly wrong code.

**Supported**

- **Path parameters** — inline scalars, or a same-document `$ref` that resolves
  to a scalar.
- **Query parameters** — scalars and arrays of scalars, lowered into a
  per-operation struct and read with [`axum-extra`]'s `Query` extractor (it
  supports the repeated keys arrays need). Required parameters stay bare;
  optional ones become `Option<..>`. A server that uses them needs
  `axum-extra = { version = "0.10", features = ["query"] }`, and array
  parameters must use OpenAPI's default `style: form`, `explode: true`
  encoding (`?tag=a&tag=b`).
- **Header parameters** — scalars only, read by a generated
  `axum::extract::FromRequestParts` impl that parses each value with `FromStr`.
  A missing required header or an unparseable value yields a `400 Bad Request`
  with a short plaintext reason. The reserved `Accept`, `Content-Type`, and
  `Authorization` headers are ignored, per the spec.
- **Cookie parameters** — scalars only, read via [`axum-extra`]'s `CookieJar`
  and lowered into a per-operation struct extracted with
  `axum::extract::FromRequestParts`. Required parameters stay bare; optional
  ones become `Option<..>`. A missing required cookie or an unparseable value
  yields a `400 Bad Request` with a short plaintext reason. A server that uses
  them needs `axum-extra = { version = "0.10", features = ["cookie"] }`.
- **Component `$ref` parameters and request bodies** —
  `#/components/parameters/*` and `#/components/requestBodies/*` are resolved
  within the same document.
- **JSON request bodies** — a `$ref` or a scalar.
- **Responses** — keyed by an explicit status code, the `default` catch-all, or
  a range (`5XX`), including component `$ref` responses
  (`#/components/responses/...`). A fixed code is emitted as a constant;
  `default`/range variants instead carry an `axum::http::StatusCode` the handler
  supplies (e.g. `GetBookResponse::Default(StatusCode::BAD_REQUEST, error)`),
  mirroring how oapi-codegen's strict server lets the handler set the code.
- **Cross-file `$ref`s** in bodies and responses are routed through
  `import-mapping` to an external module type (e.g. `crate::apimodel::Widget`).

**Rejected** (an error, never mis-generated)

- A response keyed by an unrecognised HTTP status code.
- A cross-file `$ref` parameter or request-body wrapper, or a cross-file
  component-_response_ `$ref`.
- An object or other non-scalar path, query, or header parameter.
- An array query parameter using a non-default encoding (`explode: false`,
  `spaceDelimited`, …), an array header parameter, or a `byte`/`binary` header
  parameter.
- An object or array cookie parameter, or a `byte`/`binary` cookie parameter.

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
  `parameters` (path, query, and header), `requestBody` (JSON), and `responses`
  (explicit status codes, the `default` catch-all, and ranges such as `5XX`).
- **Planned** (the remaining server/client surface): `securitySchemes`,
  `servers`, `callbacks`, and `links`.

[`axum`]: https://crates.io/crates/axum
[`axum-extra`]: https://crates.io/crates/axum-extra
[`openapiv3`]: https://crates.io/crates/openapiv3
[`quote`]: https://crates.io/crates/quote
[`syn`]: https://crates.io/crates/syn
[`prettyplease`]: https://crates.io/crates/prettyplease
