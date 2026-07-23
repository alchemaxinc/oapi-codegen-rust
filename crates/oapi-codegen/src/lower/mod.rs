//! Lowering OpenAPI documents into the intermediate representation (IR).
//!
//! [`schema`] lowers component schemas into an [`crate::ir::Module`]; [`paths`]
//! lowers operations into an [`crate::ir::Service`]. Both feed the emit pass.

pub mod paths;
pub mod prune;
pub mod rename;
pub mod schema;
pub mod security;
pub mod servers;

pub use crate::lower::paths::generate_service;
pub use crate::lower::prune::prune_unused_models;
pub use crate::lower::rename::qualify_service_models;
pub use crate::lower::rename::rewrite_service;
pub use crate::lower::rename::type_renames;
pub use crate::lower::schema::generate_models;
pub use crate::lower::servers::lower_server_urls;
