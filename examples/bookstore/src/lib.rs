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
//! Each `include!`d file is generated output: it is idiomatic (tail-expression)
//! Rust that this workspace's bespoke `implicit_return` lint does not target, and
//! it intentionally exposes types/methods this crate never constructs — hence the
//! `dead_code` and `clippy::implicit_return` allowances scoped to those modules.

/// Models generated from the schema files, one module per source file.
pub mod apimodel {
    #[allow(
        dead_code,
        clippy::implicit_return,
        reason = "generated output: idiomatic tail-expression code the implicit_return lint does not target, and this module exposes types/methods this crate never constructs"
    )]
    pub mod common {
        include!("../generated/apimodel/common.rs");
    }

    #[allow(
        dead_code,
        clippy::implicit_return,
        reason = "generated output: idiomatic tail-expression code the implicit_return lint does not target, and this module exposes types/methods this crate never constructs"
    )]
    pub mod catalog {
        include!("../generated/apimodel/catalog.rs");
    }
}

/// The axum server interface generated from `openapi.yaml`.
#[allow(
    dead_code,
    clippy::implicit_return,
    reason = "generated output: idiomatic tail-expression code the implicit_return lint does not target, and this module exposes types/methods this crate never constructs"
)]
pub mod restapi {
    include!("../generated/restapi.rs");
}

/// The blocking `reqwest` client generated from `openapi.yaml`, used by the
/// Docker e2e integration test to exercise the running server over HTTP.
#[cfg(feature = "client")]
#[allow(
    dead_code,
    clippy::implicit_return,
    reason = "generated output: idiomatic tail-expression code the implicit_return lint does not target, and this module exposes types/methods this crate never constructs"
)]
pub mod restclient {
    include!("../generated/restclient.rs");
}

mod service;

pub use crate::service::Service;
