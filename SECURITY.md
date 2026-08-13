# Security

## Report a vulnerability

Report it privately through
[GitHub security advisories](https://github.com/alchemaxinc/oapi-codegen-rust/security/advisories/new).
Do not open a public issue.

Include the OpenAPI specification and the command that shows the problem.

## Supported versions

The newest release gets the fix. Older releases get none.

## Trust the specification you give it

This tool reads an OpenAPI specification and writes Rust source. Both steps
trust that specification.

A `$ref` can name another file. The generator reads that file from the disk,
relative to the specification. A path with `..` or a leading `/` reads a file
outside that directory. The generator fetches no URL.

The output is source code that you compile. Read it, as you read any dependency.
