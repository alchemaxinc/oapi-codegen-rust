//! Integration tests for the dependency report's `Cargo.toml` parsing, driven by
//! manifest fixture files under `tests/fixtures/manifests/` rather than inline
//! strings.

use std::path::PathBuf;

/// Read a `Cargo.toml` fixture from `tests/fixtures/manifests/`.
fn manifest_fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/manifests")
        .join(name);
    return std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("reading manifest fixture `{}` failed: {err}", path.display()));
}

#[test]
fn manifest_names_span_dependency_tables_and_forms() {
    let manifest = manifest_fixture("mixed_dependency_tables.toml");
    let names = oapi_codegen::deps::manifest_dependency_names(&manifest);
    for expected in ["axum", "serde", "http", "reqwest", "uuid"] {
        assert!(names.contains(expected), "expected `{expected}` in {names:?}");
    }
    assert!(!names.contains("consumer"), "the package name is not a dependency");
}

#[test]
fn manifest_parser_ignores_inner_lines_of_multiline_entries() {
    let manifest = manifest_fixture("multiline_features.toml");
    let names = oapi_codegen::deps::manifest_dependency_names(&manifest);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(names, vec!["axum", "serde"]);
}
