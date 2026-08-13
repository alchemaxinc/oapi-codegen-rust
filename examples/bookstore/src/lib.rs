//! Composed example crate for `oapi-codegen-rust`.
//!
//! This crate is generated from the multi-file OpenAPI spec in this directory:
//! - `schemas/common.yaml` and `schemas/catalog.yaml` each become a model module
//!   under [`apimodel`] (`apimodel::common`, `apimodel::catalog`);
//! - `openapi.yaml` becomes the axum server interface in [`restapi`], whose
//!   cross-file `$ref`s resolve to the model modules via the `import-mapping`
//!   entries in `oapi-codegen-server.yaml`.
//!
//! Regenerate the modules below with `make generate-example` from the repo root.
//! Each module reads its generated file with `#[path]` and not with `include!`. A
//! generated file opens with an inner attribute that turns every clippy lint off
//! for itself, and `include!` cannot carry one.

/// Models generated from the schema files, one module per source file.
///
/// A `#[path]` on an inline module sets the directory its children resolve
/// against, so the two children below name a bare file.
#[path = "../generated/apimodel"]
pub mod apimodel {
    #[path = "common.rs"]
    pub mod common;

    #[path = "catalog.rs"]
    pub mod catalog;
}

/// The axum server interface generated from `openapi.yaml`.
#[path = "../generated/restapi.rs"]
pub mod restapi;

/// The blocking `reqwest` client generated from `openapi.yaml`, used by the
/// Docker e2e integration test to exercise the running server over HTTP.
#[cfg(feature = "client")]
#[path = "../generated/restclient.rs"]
pub mod restclient;

mod service;

pub use crate::service::Service;
