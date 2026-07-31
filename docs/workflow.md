# Build workflow

Generated code is a build input, and not a build product. Commit it, and let
continuous integration confirm that it matches the spec.

## Commit the generated file

The generator writes one Rust file. Put that file in version control next to the
hand-written code that uses it.

This has three effects. A reader of a pull request sees what the spec change did
to the API. A consumer of the crate builds it with no code generator and no
OpenAPI document. A reviewer sees a change to a public type as a change to a
committed file.

## Gate the build with `--check`

`--check` generates the code in memory and compares it with the output file. It
writes nothing.

| Result                            | Exit code |
| --------------------------------- | --------- |
| The file holds the generated code | 0         |
| The file holds different content  | 1         |
| The file does not exist           | 1         |

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
