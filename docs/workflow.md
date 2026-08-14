# Build workflow

Generated code is a build input, and not a build product. Commit it, and let
continuous integration confirm that it matches the spec.

## Commit the generated files

A run that generates operations writes a small module tree:

```text
generated/restapi.rs            the file you mount
generated/restapi/
    models.rs                   the component schemas
    server_urls.rs              the constants for the spec's servers
    operations.rs
    operations/<operation>.rs   one file per operation: inputs and responses
    server.rs                   the `Api` trait and the router
    server/<operation>.rs       one file per operation: extractors and handler
    client.rs                   `Client`, `ClientError`, and the constructors
    client/<operation>.rs       one file per operation: the request method
```

The directory takes its name from the output file. It holds only the modules the
run produces, so a client-only run writes no `server.rs`. A run that generates
models but no operations has nothing to split and writes the single file alone.

Put all of it in version control next to the hand-written code that uses it.

This has three effects. A reader of a pull request sees what the spec change did
to the API. A consumer of the crate builds it with no code generator and no
OpenAPI document. A reviewer sees a change to a public type as a change to a
committed file.

The generator owns the directory. Each run deletes the generated files in it that
the run no longer produces, so a renamed operation leaves nothing behind. Before
it writes anything, a run reads every file already in the directory and stops if
it finds one that is not its own, which is any file without the generated header
and any symbolic link. Nothing is written or deleted when a run stops that way,
so a file that lands in the directory by mistake is never lost.

Two configurations must not nest their outputs. An `output` of `generated/api.rs`
owns all of `generated/api/`, so no other configuration may write inside it.

The output path needs a `.rs` extension, because the directory is named after the
file's stem.

## Read the file with `#[path]`

The root file re-exports every module, so one declaration reads the whole tree
and every generated name keeps the same path:

```rust,ignore
#[path = "generated/restapi.rs"]
pub mod restapi;
```

`restapi::Api` and `restapi::ListBooksQuery` resolve as before, whichever file
holds them.

The generated files open with an inner attribute that turns off every lint that is
about hand-written source. Declaring a file as a module makes rustc apply that
attribute to the file alone.

Do not use `include!`. It pastes the text into the module that calls it, and rustc
rejects an inner attribute in a paste.

A `#[path]` on an inline module sets the directory that the children of the module
resolve against:

```rust,ignore
#[path = "generated/apimodel"]
pub mod apimodel {
    #[path = "common.rs"]
    pub mod common;
    #[path = "catalog.rs"]
    pub mod catalog;
}
```

A `#[path]` module is part of your crate, so `cargo fmt` walks it and reformats
it. The generated text then no longer matches what the generator writes, and
`--check` reports drift. Tell `rustfmt` to leave the directory alone in
`.rustfmt.toml`:

```toml
ignore = ["**/generated/**"]
```

## Gate the build with `--check`

`--check` generates the code in memory and compares it with the files on disk. It
writes nothing.

| Result                                          | Exit code |
| ----------------------------------------------- | --------- |
| Every file holds the generated code             | 0         |
| A file holds different content                  | 1         |
| A file does not exist                           | 1         |
| An earlier run's file is there and now unneeded | 1         |

A non-zero exit code stops the build, so a spec change with no regenerated
output cannot merge.

```sh
oapi-codegen --config-file oapi-codegen.yaml --check api.yaml
```

The flag reads the same configuration and the same spec as a normal run, so the
comparison covers every option that changes the output. `--check` adds no
dependency to your manifest, so it reports none and it rejects `--install-deps`.

## A Makefile target to copy

```make
.PHONY: generate verify-generated

generate: ## Regenerate the API from the spec
	oapi-codegen --config-file oapi-codegen.yaml api.yaml

verify-generated: ## Fail if the generated API is out of date
	oapi-codegen --config-file oapi-codegen.yaml --check api.yaml
```

Run `make verify-generated` in continuous integration, and `make generate` after
each spec change.

A GitHub Actions step needs no more than the same command:

```yaml
- name: Verify the generated API
  run: make verify-generated
```

## Why not a build script

A `build.rs` that runs the generator looks convenient. Do not do it. Four
problems apply.

The generator reads the file system. A `$ref` to another file resolves against
the directory of the spec, so a build script must know where that spec is at
build time. A crate that a consumer downloads from a registry holds no such
directory.

A build script makes the build non-hermetic. The output then depends on a spec
file outside the crate, and two builds of one crate version can give two
different APIs.

A build script hides the generated code. A reviewer sees the spec change, and not
the change to the public API that came with it. A type that disappeared is then a
compile error in a consumer, and not a line in a diff.

A build script adds the generator to every downstream build. The generator and
its dependency tree compile before the crate that uses it, for every consumer,
on every clean build.

Commit the output instead, and use `--check` to keep it correct.

## Regenerate after a version change

A new generator version can change the output for an unchanged spec, because a
fix or a new feature changes what the generator emits. `--check` reports such a
change as drift, which is the correct report: the committed file no longer
matches what the generator produces.

Pin the generator version so a build reports drift only when you choose to move.
Install an exact version, or run the generator through a lock file:

```sh
cargo install oapi-codegen --version 0.1.0 --locked
```
