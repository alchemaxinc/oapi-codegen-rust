## Support OpenAPI v{{VERSION}}

OpenAPI Specification [`{{VERSION}}`](https://github.com/OAI/OpenAPI-Specification/releases/tag/{{VERSION}}) was
published upstream. This issue was opened automatically by the `detect-openapi-release` workflow to track adding support
for it to `oapi-codegen-rust`.

The currently supported versions are listed in the
[README support matrix](https://github.com/{{REPOSITORY}}#openapi-version-support), generated from
`.github/openapi-versions.json`.

### Requested assessment

@copilot please survey the codebase and produce a **high-level overview** of what would need to change to support
OpenAPI v{{VERSION}}. Do not implement the changes yet — focus on scoping. Please cover, at minimum:

- **Loader / parser** (`crates/oapi-codegen/src/loader.rs`): new document shapes, fields, or `$ref` semantics.
- **Lowering** (`crates/oapi-codegen/src/lower/`): schema, servers, parameters, and any new constructs.
- **Naming & code generation**: whether new keywords or types affect identifier generation or the emitted Rust.
- **Validation**: new or changed constraints to enforce or reject.
- **Test fixtures**: coverage specs and generated snapshots to add or update (many fixtures pin `openapi: 3.0.3`).
- **Docs**: the README support matrix (via `.github/openapi-versions.json`) and anything under `docs/`.

Summarise the estimated scope (small / medium / large) and call out any breaking changes relative to the currently
supported versions.

### When support lands

Mark `{{VERSION}}` as `supported` in `.github/openapi-versions.json` with the release tag in `supported_since`, run
`make update-versions`, and close this issue.
