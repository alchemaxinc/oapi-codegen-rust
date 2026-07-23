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

### `output-options`

| Key                     | Purpose                                                                                             |
| ----------------------- | --------------------------------------------------------------------------------------------------- |
| `skip-prune`            | Keep schemas not referenced by any retained operation/schema.                                       |
| `include-tags`          | Only generate operations with one of these tags.                                                    |
| `exclude-tags`          | Skip operations with any of these tags.                                                             |
| `include-operation-ids` | Only generate these `operationId`s.                                                                 |
| `exclude-operation-ids` | Skip these `operationId`s.                                                                          |
| `exclude-schemas`       | Drop these component schemas before lowering.                                                       |
| `response-type-suffix`  | Suffix for response enums (default `Response`); set it to resolve a clash with a same-named schema. |

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
