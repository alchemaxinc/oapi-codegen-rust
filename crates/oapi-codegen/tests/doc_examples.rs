//! Executes the `oapi-codegen` invocations shown in the project's Markdown
//! docs and asserts their output matches what's committed.
//!
//! Each fenced ` ```console ` block is parsed and actually run against the
//! built `oapi-codegen` binary (`$` starts a command, `?` its expected exit
//! status, the following lines its expected stdout+stderr). This means a
//! change to CLI parsing (a new required flag, a renamed option, a different
//! error message) breaks this test instead of silently leaving the docs
//! stale. Fenced blocks tagged with any other language (e.g. ` ```sh `) are
//! ignored by `trycmd`, so illustrative snippets that aren't meant to be
//! executed as-is (placeholders, `cargo install`, `git clone`, ...) are kept
//! that way on purpose.
//!
//! Run `TRYCMD=overwrite cargo test -p oapi-codegen --test doc_examples` to
//! refresh expected output after an intentional CLI change.

#[test]
fn doc_examples_match_cli_behavior() {
    trycmd::TestCases::new()
        .case("../../README.md")
        .case("../../docs/installation.md");
}
