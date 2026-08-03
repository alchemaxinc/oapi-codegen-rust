//! Compile every committed fixture on the Rust version a consumer needs.
//!
//! The generated-code floor is declared once, under
//! `[package.metadata.generated-code]` in `crates/oapi-codegen/Cargo.toml`. That
//! number is reported to every consumer after a write, so it must be true rather
//! than aspirational. A dependency that raises its own floor raises this one
//! without touching a line of the generator, so the claim needs a check that runs
//! on the toolchain it names.
//!
//! There are no assertions here. Compiling is the assertion: if a fixture
//! needs a newer `rustc` than the declared floor, this crate does not build.
//!
//! Run it with `make verify-msrv`. See `docs/msrv.md`.

// Generated code is ordinary idiomatic Rust, and the workspace lint set targets
// first-party source. This crate holds nothing but generated code, so the
// exceptions cover the whole of it.
#![allow(dead_code, reason = "a fixture declares types this crate never constructs")]
#![allow(clippy::all, reason = "generated code is not linted against the workspace rules")]

/// Stand-in for the models crate two fixtures point their cross-file `$ref`
/// bodies at. The emitted path is `crate::apimodel`, so it must sit at this
/// crate's root. The body is shared with `tests/generated.rs`, which is now a
/// sibling two levels up.
mod apimodel {
    include!("../../support/apimodel.rs");
}

include!(concat!(env!("OUT_DIR"), "/fixtures.rs"));
