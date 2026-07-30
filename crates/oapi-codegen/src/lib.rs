//! `oapi-codegen` — generate idiomatic Rust from OpenAPI 3 specifications.
//!
//! The pipeline is: load a spec ([`loader`]), lower its component schemas into
//! an intermediate representation ([`lower::schema`] → [`ir`]) and, for the server
//! generator, its operations ([`lower::paths`] → [`ir`]), then emit formatted Rust
//! source ([`emit`]). [`Config`] mirrors `oapi-codegen`'s YAML configuration.

pub mod cli;
pub mod config;
pub mod deps;
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

/// Generate Rust from a spec file according to `configuration`, returning the source.
///
/// Models are emitted when `generate.models` is set, or implicitly when the
/// server or client is generated (so referenced types are in scope). The axum
/// server interface is appended when `generate.std-http-server` is set. The
/// blocking `reqwest` client is appended when `generate.client` is set. Models,
/// per-operation types, and both generators are emitted flat at the crate root,
/// so server and client can share one file.
pub fn generate(spec_path: &Path, config: &Config) -> Result<String> {
    if config.generate.embedded_spec {
        return Err(Error::Unimplemented("embedded-spec".to_owned()));
    }
    let mut spec = Spec::load(spec_path)?;
    spec.apply_filters(&config.output_options);
    let want_server = config.generate.std_http_server;
    let want_client = config.generate.client;
    let server_urls = if config.generate.server_urls {
        lower::lower_server_urls(&spec)?
    } else {
        None
    };
    // A run that emits no type resolves no type name. `server-urls` on its own
    // emits constants only, so it must not read `type-name-suffix` and must not
    // report a collision between two schemas that it never looks at. This matches
    // `response-type-suffix`, which only a server or client run reads.
    if !(config.generate.models || want_server || want_client) {
        return emit::emit_module(&Module::default(), server_urls.as_ref());
    }
    // A set but useless suffix is an error, and not silently "unset". The
    // resolution checks it, so every caller of `type_renames` gets the check.
    // See `lower::rename::checked_suffix`.
    let type_name_suffix = config.output_options.type_name_suffix.as_deref();
    // Resolve the type names one time and share them. An unresolved collision is
    // held inside `names` and reported below, after pruning decides which models
    // the file holds.
    let names = lower::type_renames(&spec, type_name_suffix)?;
    let mut module = lower::generate_models(&spec, &names)?;
    if want_server || want_client {
        let response_type_suffix = config
            .output_options
            .response_type_suffix
            .as_deref()
            .filter(|suffix| return !suffix.is_empty())
            .unwrap_or(crate::config::DEFAULT_RESPONSE_SUFFIX);
        let mut service = lower::generate_service(&spec, &config.import_mapping, response_type_suffix)?;
        lower::rewrite_service(&mut service, names.renames());
        if !config.output_options.skip_prune {
            lower::prune_unused_models(&mut module, &service);
        }
        // The module is final here, so a collision between two pruned schemas is
        // no longer a problem and only a surviving one is reported. With
        // `skip-prune` the module holds every schema, so every collision reports.
        names.check_emitted(&module)?;
        let targets = emit::Targets {
            server: want_server,
            client: want_client,
        };
        lower::check_type_name_collisions(&service, &module, &emit::reserved_type_names(targets))?;
        return emit::emit_flat(&module, &service, server_urls.as_ref(), targets);
    }
    // Models-only generation prunes nothing, so the module holds every schema and
    // every collision reports.
    names.check_emitted(&module)?;
    return emit::emit_module(&module, server_urls.as_ref());
}

/// Generate Rust from a spec file according to `configuration` and write it to
/// `output_path`, creating parent directories as needed.
pub fn generate_to_file(spec_path: &Path, config: &Config, output_path: &Path) -> Result<()> {
    let code = generate(spec_path, config)?;
    return write_output(output_path, &code);
}

/// Generate Rust models from a spec file and return the formatted source.
///
/// This entry point takes no config, so two schema names that collapse onto one
/// Rust identifier are an error. Every schema becomes an item, because no
/// operation exists to prune against. Use [`generate`] with
/// `output-options.type-name-suffix` to resolve such a collision by config.
pub fn generate_models_string(spec_path: &Path) -> Result<String> {
    let spec = Spec::load(spec_path)?;
    let names = lower::type_renames(&spec, None)?;
    let module = lower::generate_models(&spec, &names)?;
    // Every schema becomes an item here, so every collision reaches the file.
    names.check_emitted(&module)?;
    let code = emit::emit_module(&module, None)?;
    return Ok(code);
}

/// Generate Rust models from a spec file and write them to `output_path`,
/// creating parent directories as needed.
pub fn generate_models_to_file(spec_path: &Path, output_path: &Path) -> Result<()> {
    let code = generate_models_string(spec_path)?;
    return write_output(output_path, &code);
}

/// Write generated source to `output_path`, creating parent directories.
pub fn write_output(output_path: &Path, code: &str) -> Result<()> {
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
