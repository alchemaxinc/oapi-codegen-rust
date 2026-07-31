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
        // A hoisted inline type carries no component name, so the resolution pass
        // above cannot see it. The final item names can still hold a duplicate.
        lower::check_duplicate_models(&module)?;
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
    lower::check_duplicate_models(&module)?;
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
    lower::check_duplicate_models(&module)?;
    let code = emit::emit_module(&module, None)?;
    return Ok(code);
}

/// Generate Rust models from a spec file and write them to `output_path`,
/// creating parent directories as needed.
pub fn generate_models_to_file(spec_path: &Path, output_path: &Path) -> Result<()> {
    let code = generate_models_string(spec_path)?;
    return write_output(output_path, &code);
}

/// What a comparison of generated code against an output file found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drift {
    /// The output file holds the generated code.
    None,
    /// The output file does not exist. Generation creates it, so this is drift
    /// and not a read failure.
    Absent,
    /// The output file exists and holds different content.
    Differs,
}

/// Compare `code` with the content of `output_path` and report the difference.
///
/// This reads the file and writes nothing, so a caller can gate a build on stale
/// generated code. The comparison is exact, because the generator formats every
/// output through `prettyplease` and therefore produces one byte sequence for one
/// input.
///
/// The comparison reads bytes and not text. Generated Rust is always UTF-8, so a
/// file that is not gives [`Drift::Differs`]. That is what the file is, and it
/// also keeps a hand-edited or truncated file on the drift path where the remedy
/// applies, rather than on the error path where it does not.
///
/// # Errors
///
/// Returns [`Error::ReadOutput`] when the file exists and cannot be read, for
/// example a directory in place of a file. An absent file gives [`Drift::Absent`]
/// and not an error, because generation creates it.
pub fn check_output(output_path: &Path, code: &str) -> Result<Drift> {
    let existing = match std::fs::read(output_path) {
        Ok(existing) => existing,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Drift::Absent);
        }
        Err(source) => {
            return Err(Error::ReadOutput {
                path: output_path.display().to_string(),
                source,
            });
        }
    };
    if existing == code.as_bytes() {
        return Ok(Drift::None);
    }
    return Ok(Drift::Differs);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory under the temp directory that goes away with the test.
    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(test_name: &str) -> Self {
            let unique = format!(
                "oapi-codegen-check-{test_name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock should be after Unix epoch")
                    .as_nanos(),
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).expect("create test directory");
            return Self { path };
        }

        fn join(&self, file: &str) -> std::path::PathBuf {
            return self.path.join(file);
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn check_output_reports_an_absent_file_as_drift() {
        let dir = TestDir::new("absent");
        // Generation creates the file, so absence is drift and not a read failure.
        let drift = check_output(&dir.join("out.rs"), "pub struct Widget;\n").expect("check an absent file");
        assert_eq!(drift, Drift::Absent);
    }

    #[test]
    fn check_output_reports_equal_content_as_no_drift() {
        let dir = TestDir::new("equal");
        let path = dir.join("out.rs");
        let code = "pub struct Widget;\n";
        std::fs::write(&path, code).expect("write the output file");
        let drift = check_output(&path, code).expect("check an equal file");
        assert_eq!(drift, Drift::None);
    }

    #[test]
    fn check_output_reports_different_content_as_drift() {
        let dir = TestDir::new("differs");
        let path = dir.join("out.rs");
        std::fs::write(&path, "pub struct Widget;\n").expect("write the output file");
        let drift = check_output(&path, "pub struct Gadget;\n").expect("check a stale file");
        assert_eq!(drift, Drift::Differs);
    }

    #[test]
    fn check_output_compares_exactly() {
        let dir = TestDir::new("exact");
        let path = dir.join("out.rs");
        // The generator formats every output, so one input gives one byte
        // sequence. A trailing newline is therefore a real difference and not
        // noise to normalise away.
        std::fs::write(&path, "pub struct Widget;").expect("write the output file");
        let drift = check_output(&path, "pub struct Widget;\n").expect("check a file with no trailing newline");
        assert_eq!(drift, Drift::Differs);
    }

    #[test]
    fn check_output_reports_content_that_is_not_utf8_as_drift() {
        let dir = TestDir::new("not-utf8");
        let path = dir.join("out.rs");
        // Generated Rust is always UTF-8, so such a file is a differing file and
        // not an unreadable one. The remedy for drift applies, and the remedy for
        // a read failure does not.
        std::fs::write(&path, [0xFF_u8, 0xFE_u8]).expect("write the output file");
        let drift = check_output(&path, "pub struct Widget;\n").expect("check a file that is not UTF-8");
        assert_eq!(drift, Drift::Differs);
    }

    #[test]
    fn check_output_writes_nothing() {
        let dir = TestDir::new("readonly");
        let path = dir.join("out.rs");
        let existing = "pub struct Widget;\n";
        std::fs::write(&path, existing).expect("write the output file");
        let drift = check_output(&path, "pub struct Gadget;\n").expect("check a stale file");
        assert_eq!(drift, Drift::Differs);
        let after = std::fs::read_to_string(&path).expect("read the output file back");
        assert_eq!(after, existing, "`check_output` must not change the file");
    }

    #[test]
    fn check_output_fails_on_a_path_it_cannot_read() {
        let dir = TestDir::new("unreadable");
        let path = dir.join("out.rs");
        // A directory exists but holds no string content, so this is a read
        // failure and not drift.
        std::fs::create_dir(&path).expect("create a directory where a file belongs");
        let error = check_output(&path, "pub struct Widget;\n").expect_err("a directory is not readable as a file");
        assert!(
            matches!(error, Error::ReadOutput { .. }),
            "expected `ReadOutput`, got {error:?}"
        );
    }
}
