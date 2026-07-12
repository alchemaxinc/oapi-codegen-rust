//! `oapi-codegen` — generate idiomatic Rust from OpenAPI 3 specifications.
//!
//! The pipeline is: load a spec ([`loader`]), lower its component schemas into
//! an intermediate representation ([`lower::schema`] → [`ir`]) and, for the server
//! generator, its operations ([`lower::paths`] → [`ir`]), then emit formatted Rust
//! source ([`emit`]). [`Config`] mirrors `oapi-codegen`'s YAML configuration.

pub mod config;
pub mod emit;
pub mod error;
pub mod filter;
pub mod ir;
pub mod loader;
pub mod lower;
pub mod naming;

use std::path::Path;

pub use crate::config::Config;
pub use crate::error::Error;
pub use crate::error::Result;
use crate::ir::Module;
use crate::loader::Spec;

/// Generate Rust from a spec file according to `config`, returning the source.
///
/// Models are emitted when `generate.models` is set, or implicitly when the
/// server or client is generated (so referenced types are in scope). The axum
/// server interface is appended when `generate.std-http-server` is set; the
/// blocking `reqwest` client is appended when `generate.client` is set.
pub fn generate(spec_path: &Path, config: &Config) -> Result<String> {
    let mut spec = Spec::load(spec_path)?;
    spec.apply_filters(&config.output_options);
    let want_server = config.generate.std_http_server;
    let want_client = config.generate.client;
    let mut module = if config.generate.models || want_server || want_client {
        lower::generate_models(&spec)?
    } else {
        Module::default()
    };
    if want_server {
        let mut service = lower::generate_service(&spec, &config.import_mapping)?;
        lower::rewrite_service(&mut service, &lower::type_renames(&spec));
        if !config.output_options.skip_prune {
            lower::prune_unused_models(&mut module, &service);
        }
        return emit::emit_with_service(&module, &service);
    }
    if want_client {
        let mut service = lower::generate_service(&spec, &config.import_mapping)?;
        lower::rewrite_service(&mut service, &lower::type_renames(&spec));
        if !config.output_options.skip_prune {
            lower::prune_unused_models(&mut module, &service);
        }
        return emit::emit_with_client(&module, &service);
    }
    return emit::emit_module(&module);
}

/// Generate Rust from a spec file according to `config` and write it to
/// `output_path`, creating parent directories as needed.
pub fn generate_to_file(spec_path: &Path, config: &Config, output_path: &Path) -> Result<()> {
    let code = generate(spec_path, config)?;
    return write_output(output_path, &code);
}

/// Generate Rust models from a spec file and return the formatted source.
pub fn generate_models_string(spec_path: &Path) -> Result<String> {
    let spec = Spec::load(spec_path)?;
    let module = lower::generate_models(&spec)?;
    let code = emit::emit_module(&module)?;
    return Ok(code);
}

/// Generate Rust models from a spec file and write them to `output_path`,
/// creating parent directories as needed.
pub fn generate_models_to_file(spec_path: &Path, output_path: &Path) -> Result<()> {
    let code = generate_models_string(spec_path)?;
    return write_output(output_path, &code);
}

/// Write generated source to `output_path`, creating parent directories.
fn write_output(output_path: &Path, code: &str) -> Result<()> {
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
