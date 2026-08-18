# Contributing

## Before you write code

Open an issue first for a new feature or a change of behavior. A short
discussion saves a large rewrite.

## The loop

```sh
make help    # every target
make lint    # clippy, rustdoc, rustfmt, prettier
make test-unit
make verify-generated
```

`make lint` runs `cargo fmt` on nightly. `rust-toolchain.toml` pins the rest.

## The repository holds generated files

A change to the generator changes its own output. Three paths hold that output:

- `crates/oapi-codegen/tests/generated`
- `examples/bookstore/generated`
- `docs/cli.md`

Run `make verify-generated` before you open a pull request. It regenerates all
three and fails on a difference. CI runs the same target.

## A new construct needs a row and a fixture

`TEST_TABLE` in `crates/oapi-codegen/tests/coverage.rs` lists every OpenAPI 3.0
construct. Each one is Supported, Unsupported, Ignored, or Planned. Nothing is
unknown, and a test fails when a construct has no entry.

A new fixture needs four registrations, or the tests fail:

1. Its row in `TEST_TABLE`.
2. Its stem in the matching fixture list, such as `COMBINED_FIXTURES`.
3. Its stem in the matching `generated_tests!` macro call.
4. Its module in `tests/generated.rs`.

## Commits and branches

Base your branch on `develop`. Only `main` carries stable releases.

Write commit messages in the
[Conventional Commits](https://www.conventionalcommits.org) form. `commitlint`
checks them, and each line has a limit of 150 characters.

Dependency updates use the `deps` scope. Cargo updates use `build(deps)`, and
GitHub Actions updates use `ci(deps)`. semantic-release groups these commits
under `Dependency Updates`.

## Bug reports

Give the smallest specification that shows the problem. This tool reads a
specification, so a report without one is hard to act on.
