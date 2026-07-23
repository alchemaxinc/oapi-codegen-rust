<!--
Template for the tracking issue opened by the detect-openapi-release workflow
when a new OpenAPI Specification version is published upstream. The workflow
substitutes the `{{...}}` placeholders before creating the issue and assigning
it to Copilot for a code-wide assessment. Keep the assessment prompt precise so
Copilot can survey the whole repository consistently.
-->

## Support OpenAPI v{{VERSION}}

OpenAPI Specification [`{{TAG}}`]({{RELEASE_URL}}) was published upstream on
{{PUBLISHED_AT}}. This issue was opened automatically by the
`detect-openapi-release` workflow to track adding support for it to
`oapi-codegen-rust`.

- Upstream release: {{RELEASE_URL}}
- Specification version: `{{VERSION}}`
- Currently supported versions are listed in the
  [README support matrix](https://github.com/{{REPOSITORY}}#openapi-version-support),
  generated from `.github/openapi-versions.json`.

### Requested assessment

@copilot please survey the codebase and produce a **high-level overview** of
what would need to change to support OpenAPI v{{VERSION}}. Do not implement the
changes yet — focus on scoping. Please cover, at minimum:

- **Loader / parser** (`crates/oapi-codegen/src/loader.rs`): what new document
  shapes, fields, or `$ref` semantics the new version introduces.
- **Lowering** (`crates/oapi-codegen/src/lower/`): schema, servers, parameters,
  and any new constructs that must be represented in the intermediate model.
- **Naming & code generation**: whether new keywords or types affect identifier
  generation or the emitted Rust.
- **Validation**: new or changed constraints that need enforcing or rejecting.
- **Test fixtures**: coverage specs and generated snapshots that must be added
  or updated (note the many `openapi: 3.0.3` fixtures that pin the version).
- **Docs**: `README.md` support matrix (via `.github/openapi-versions.json`)
  and anything under `docs/` that references version support.

Summarise the estimated scope (small / medium / large) and call out any breaking
changes relative to the currently supported versions.

### When support lands

Update `.github/openapi-versions.json` to mark `{{VERSION}}` as `supported` with
the release tag in `supported_since`, run `make update-versions`, and close this
issue.
