//! Coverage guard for the e2e suite.
//!
//! Every operation declared in `openapi.yaml` must have a registered e2e test.
//! Unlike `tests/e2e.rs`, this file is **not** behind the `client` feature and
//! needs no running server, so it executes in the default `cargo test` gate:
//! adding an endpoint to the spec fails CI until it is both implemented and
//! covered.
//!
//! When you add an operation to `openapi.yaml`, add a matching `#[test]` in
//! `tests/e2e.rs` and its `operationId` to [`TESTED_OPERATIONS`] below.

use std::collections::BTreeSet;

/// Operations exercised by `tests/e2e.rs`, keyed by their OpenAPI `operationId`.
///
/// Keep this in lockstep with `openapi.yaml`: the test below fails if the spec
/// declares an operation missing here (untested endpoint) or if an entry here no
/// longer exists in the spec (stale registration).
const TESTED_OPERATIONS: &[&str] = &[
    "listBooks",
    "createBook",
    "getBook",
    "uploadBookCover",
    "submitReview",
    "getHealth",
];

#[test]
fn every_operation_has_a_registered_e2e_test() {
    let document: serde_yaml::Value =
        serde_yaml::from_str(include_str!("../openapi.yaml")).expect("parse openapi.yaml");
    let declared = declared_operation_ids(&document);
    let tested: BTreeSet<String> = TESTED_OPERATIONS
        .iter()
        .map(|operation| {
            return (*operation).to_owned();
        })
        .collect();

    let untested: Vec<&String> = declared.difference(&tested).collect();
    assert!(
        untested.is_empty(),
        "operations in openapi.yaml with no registered e2e test — add a test in \
         tests/e2e.rs and an entry in TESTED_OPERATIONS: {untested:?}",
    );

    let stale: Vec<&String> = tested.difference(&declared).collect();
    assert!(
        stale.is_empty(),
        "TESTED_OPERATIONS entries that no longer exist in openapi.yaml — remove \
         them: {stale:?}",
    );
}

/// Collect every `operationId` declared under `paths.<path>.<method>` in the
/// OpenAPI document. Non-operation path-item entries (e.g. `parameters`) carry
/// no `operationId` and are skipped.
fn declared_operation_ids(document: &serde_yaml::Value) -> BTreeSet<String> {
    let paths = match document.get("paths").and_then(|paths| {
        return paths.as_mapping();
    }) {
        Some(paths) => paths,
        None => {
            return BTreeSet::new();
        }
    };

    let mut ids = BTreeSet::new();
    for (_path, item) in paths {
        let methods = match item.as_mapping() {
            Some(methods) => methods,
            None => {
                continue;
            }
        };
        for (_method, operation) in methods {
            if let Some(id) = operation.get("operationId").and_then(|id| {
                return id.as_str();
            }) {
                ids.insert(id.to_owned());
            }
        }
    }
    return ids;
}
