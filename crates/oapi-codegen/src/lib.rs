//! `oapi-codegen` — generate idiomatic Rust from OpenAPI 3 specifications.
//!
//! The pipeline is: load a spec ([`loader`]), lower its component schemas into
//! an intermediate representation ([`schema`] → [`ir`]), and emit formatted Rust
//! source ([`emit`]). [`Config`] mirrors `oapi-codegen`'s YAML configuration.

pub mod config;
pub mod emit;
pub mod error;
pub mod ir;
pub mod loader;
pub mod naming;
pub mod schema;

use std::path::Path;

pub use crate::config::Config;
pub use crate::error::Error;
pub use crate::error::Result;
use crate::loader::Spec;

/// Generate Rust models from a spec file and return the formatted source.
pub fn generate_models_string(spec_path: &Path) -> Result<String> {
    let spec = Spec::load(spec_path)?;
    let module = schema::generate_models(&spec)?;
    let code = emit::emit_module(&module)?;
    return Ok(code);
}

/// Generate Rust models from a spec file and write them to `output_path`,
/// creating parent directories as needed.
pub fn generate_models_to_file(spec_path: &Path, output_path: &Path) -> Result<()> {
    let code = generate_models_string(spec_path)?;
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|source| {
            return Error::WriteOutput {
                path: output_path.display().to_string(),
                source,
            };
        })?;
    }
    std::fs::write(output_path, code).map_err(|source| {
        return Error::WriteOutput {
            path: output_path.display().to_string(),
            source,
        };
    })?;
    return Ok(());
}
