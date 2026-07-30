# Configuration

## CLI

```sh
oapi-codegen [OPTIONS] --config-file <CONFIG_FILE> <SPEC_FILE>
```

- `<SPEC_FILE>` — path to the OpenAPI 3 document (YAML or JSON). Required.
- `--config-file, -c` — path to a YAML config file. Required, and must enable at
  least one artifact under `generate:` (`models`, `std-http-server`, `client`,
  or `server-urls`); the tool errors with guidance otherwise.
- `--output-file, -o` — output file. Required unless the config sets `output:`
  (`--output-file` overrides it); the tool errors if neither is given. If
  generation produces no code, nothing is written and the tool exits with an
  explanation.

For a full, auto-generated reference of every flag and argument, see
[`cli.md`](./cli.md) (regenerate with `make update-docs`).

## Config file

Keys mirror [`oapi-codegen`](https://github.com/oapi-codegen/oapi-codegen)'s
YAML config; unknown keys are ignored, so an existing Go config can be reused
as-is. Only the subset below is interpreted.

| Key              | Type   | Purpose                                                                |
| ---------------- | ------ | ---------------------------------------------------------------------- |
| `package`        | string | Target module name (informational).                                    |
| `output`         | path   | Output file path.                                                      |
| `import-mapping` | map    | Map referenced spec files to external Rust modules (multi-file specs). |

### `generate`

| Key               | Purpose                                                |
| ----------------- | ------------------------------------------------------ |
| `models`          | Emit structs/enums from component schemas.             |
| `std-http-server` | Emit an axum server interface (`trait Api` + router).  |
| `client`          | Emit a blocking `reqwest` client.                      |
| `server-urls`     | Emit constants/builders for the spec's `servers` URLs. |
| `embedded-spec`   | _Not implemented_ — rejected if set.                   |

Setting both `std-http-server` and `client` emits models, per-operation types,
and both interfaces flat at the crate root, so the server and client share one
file and the same response types.

## Dependencies

Cargo does not infer crate dependencies from a generated file's `use` paths (the
way Go's `go mod tidy` does), so after each run the CLI reports the crates — with
versions and features — that the generated code references, as both a
`Cargo.toml` snippet and `cargo add` commands. Pass `--install-deps` to run those
`cargo add` commands automatically without prompting; otherwise, on an
interactive terminal you are prompted first. `cargo add` targets the package
whose `Cargo.toml` is nearest the output and merges with any existing
declaration, so a crate you already depend on is updated in place rather than
duplicated. The report lists every referenced crate rather than reading your
manifest to prune ones you already have — interpreting a manifest (workspace
inheritance, dev/target scopes, feature sufficiency) is Cargo's job.

The recommended versions are taken from `oapi-codegen`'s own manifest, so they
match the versions the generated code is built and tested against.

The set is per generated file. For example, a models-only file typically needs
only `serde`; an axum server also needs `axum`, `http`, and (with query or cookie
parameters) `axum-extra`; a `reqwest` client needs `reqwest`, `percent-encoding`,
and, for form responses, `serde_urlencoded`. `chrono`, `uuid`, and `serde_json`
are added when the schemas use dates, UUIDs, or free-form values.

### `output-options`

| Key                     | Purpose                                                                                             |
| ----------------------- | --------------------------------------------------------------------------------------------------- |
| `skip-prune`            | Keep schemas not referenced by any retained operation/schema.                                       |
| `include-tags`          | Only generate operations with one of these tags.                                                    |
| `exclude-tags`          | Skip operations with any of these tags.                                                             |
| `include-operation-ids` | Only generate these `operationId`s.                                                                 |
| `exclude-operation-ids` | Skip these `operationId`s.                                                                          |
| `exclude-schemas`       | Drop these component schemas before lowering.                                                       |
| `response-type-suffix`  | Suffix for response enums (default `Response`). Set it to resolve a clash with a same-named schema. |
| `type-name-suffix`      | Suffix for a schema name that collapses onto a Rust type name another schema already has.           |

### `type-name-suffix`

Two different schema names can produce one Rust type name. For example, `order-item` and `orderItem` both produce
`OrderItem`. The default behavior is an error, because the generator does not choose which of the two schemas keeps the
plain name. That choice belongs to the spec author.

Two remedies exist:

1. Put `x-rust-name` on one of the two schemas. This is the preferred remedy. It records the type name the author wants,
   and it changes that one schema only. See [vendor extensions](extensions.md).
2. Set `type-name-suffix`. The generator then adds the suffix to each schema after the first that collides. With
   `type-name-suffix: Alt`, the two schemas above become `OrderItem` and `OrderItemAlt`. A third collision becomes
   `OrderItemAltAlt`.

The order is document order. The first schema in the file keeps the plain name.

Only a collision that reaches the generated file is an error. Default pruning drops a schema that no generated
operation uses, so two unused schemas that collapse onto one name do not stop generation. `skip-prune` keeps every
schema, and then every collision is an error.

A run that emits no type reads neither this key nor the collision rule. `server-urls` on its own emits constants, so it
generates whatever the schemas hold.

The suffix must hold at least one letter or digit. Casing removes punctuation and separators, so a suffix such as `-`
or `_` leaves the type name unchanged and cannot resolve a collision. Such a suffix is an error. A suffix that mixes
punctuation with letters is fine, because `-v2` reduces to `V2`. To make a collision an error instead, remove the key.

No key does the same for a method name. Two `operationId`s that collapse onto one Rust name are always an error, and
`x-rust-name` on one of the two operations is the only remedy. A consumer writes each method name into an `impl` block,
so every method name stays the choice of the spec author. See [operation names](extensions.md#operation-names).

An operation that `exclude-operation-ids` or a tag filter removes claims no method name, because filtering runs before
lowering. Such an operation joins no collision.

## Example

```yaml
package: restapi
output: generated/restapi.rs
generate:
  std-http-server: true
  models: true
  server-urls: true
import-mapping:
  schemas/common.yaml: crate::apimodel::common
```
