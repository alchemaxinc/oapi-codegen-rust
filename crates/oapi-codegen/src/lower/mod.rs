//! Lowering OpenAPI documents into the intermediate representation (IR).
//!
//! [`schema`] lowers component schemas into an [`crate::ir::Module`]; [`paths`]
//! lowers operations into an [`crate::ir::Service`]. Both feed the emit pass.

pub mod paths;
pub mod schema;
pub mod security;

pub use crate::lower::paths::generate_service;
pub use crate::lower::schema::generate_models;
