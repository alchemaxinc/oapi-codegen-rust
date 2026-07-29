# Configuration

## CLI

```sh
oapi-codegen [OPTIONS] --config-file <CONFIG_FILE> <SPEC_FILE>
```

- `<SPEC_FILE>` — Path to the OpenAPI 3 document (YAML or JSON). Required.
- `--config-file, -c` — Path to a YAML configuration file. Required.
- `--output-file, -o` — Output file path. Required unless the config sets `output:`.

The configuration file must enable at least one artifact under `generate:`:
`models`, `std-http-server`, `client`, or `server-urls`.

For the full generated reference, read [`cli.md`](./cli.md).
Run `make update-docs` after you change the CLI.

## Config file

The YAML keys match [`oapi-codegen`](https://github.com/oapi-codegen/oapi-codegen).
Unknown keys are ignored. Only the keys below are used.

| Key              | Type   | Purpose                                                                |
| ---------------- | ------ | ---------------------------------------------------------------------- |
| `package`        | string | Target module name (informational).                                    |
| `output`         | path   | Output file path.                                                      |
| `import-mapping` | map    | Map referenced spec files to external Rust modules (multi-file specs). |

### `generate`

| Key               | Purpose                                                |
| ----------------- | ------------------------------------------------------ |
| `models`          | Generate structs and enums from component schemas.     |
| `std-http-server` | Generate an axum server interface (`trait Api` + router). |
| `client`          | Generate a blocking `reqwest` client.                  |
| `server-urls`     | Generate constants/builders for `servers` URLs.        |
| `embedded-spec`   | _Not implemented_. The tool rejects this key.          |

If you set both `std-http-server` and `client`, the tool generates one shared
file at the crate root. The file contains shared types for each operation.

## Dependencies

Cargo does not infer dependencies from `use` paths in generated files.
After generation, the CLI prints the needed crates, versions, and features.
It prints a `Cargo.toml` snippet and `cargo add` commands.

Use `--install-deps` to run those `cargo add` commands automatically.
Interactive terminals show a prompt without that flag.
Non-interactive runs print only the dependency list.

`cargo add` targets the package whose `Cargo.toml` is nearest the output file.
If the dependency already exists, Cargo updates it in place.

Recommended versions come from the `oapi-codegen` manifest.
They match the versions used for build and test.

Dependencies depend on the generated output.
For example, model-only output usually needs `serde`.
An axum server also needs `axum`, `http`, and sometimes `axum-extra`.
A `reqwest` client needs `reqwest`, `percent-encoding`, and sometimes
`serde_urlencoded`.

### `output-options`

| Key                     | Purpose                                                                                             |
| ----------------------- | --------------------------------------------------------------------------------------------------- |
| `skip-prune`            | Keep schemas that no kept operation/schema references.                                               |
| `include-tags`          | Generate only operations with one of these tags.                                                    |
| `exclude-tags`          | Skip operations with any of these tags.                                                             |
| `include-operation-ids` | Generate only these `operationId` values.                                                           |
| `exclude-operation-ids` | Skip these `operationId` values.                                                                    |
| `exclude-schemas`       | Remove these component schemas before lowering.                                                     |
| `response-type-suffix`  | Suffix for response enums (default `Response`). Use it to resolve a name clash with a schema name. |

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
