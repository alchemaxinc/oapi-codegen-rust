//! Command-line entry point for the `oapi-codegen` generator.
#![allow(
    clippy::print_stderr,
    reason = "this binary reports status and guided errors to stderr"
)]

mod console;

use std::io::IsTerminal;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitCode;

use clap::Parser;
use oapi_codegen::Config;
use oapi_codegen::Error;
use oapi_codegen::PackageDrift;
use oapi_codegen::Result;
use oapi_codegen::cli::Cli;
use oapi_codegen::config::Generate;

use crate::console::SpecStats;

/// A failure that has enough context to be reported to the user.
enum CliFailure {
    /// A generator/loader error occurred.
    Generator(Error),
    /// The configuration parsed but enabled no artifacts.
    NoArtifacts {
        /// Path to the offending configuration file.
        config: PathBuf,
    },
    /// No output destination was given on the CLI or in the configuration.
    NoOutput,
    /// Generation succeeded but produced no code.
    EmptyOutput {
        /// The spec that was processed.
        spec: PathBuf,
        /// Counts explaining why nothing was generated.
        stats: SpecStats,
        /// What the configuration requested.
        generate: Generate,
    },
    /// `--check` found the output file missing or out of date. Nothing was
    /// written, because the flag asks for a comparison only.
    Drift {
        /// The file the comparison stopped at.
        path: PathBuf,
        /// What the comparison found there.
        kind: console::DriftKind,
    },
}

impl CliFailure {
    /// Print a guided report for this failure to stderr.
    fn report(&self) {
        match self {
            CliFailure::Generator(err) => {
                console::report_error(err);
            }
            CliFailure::NoArtifacts { config } => {
                console::report_no_artifacts(config);
            }
            CliFailure::NoOutput => {
                console::report_no_output();
            }
            CliFailure::EmptyOutput { spec, stats, generate } => {
                console::report_empty_output(spec, stats, generate);
            }
            CliFailure::Drift { path, kind } => {
                console::report_drift(path, *kind);
            }
        }
    }
}

impl From<Error> for CliFailure {
    fn from(err: Error) -> Self {
        return CliFailure::Generator(err);
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => {
            return ExitCode::SUCCESS;
        }
        Err(failure) => {
            failure.report();
            return ExitCode::FAILURE;
        }
    }
}

/// Run the generator according to the parsed CLI arguments.
fn run(cli: &Cli) -> std::result::Result<(), CliFailure> {
    let config = Config::load(&cli.config_file)?;

    if config.generate.embedded_spec {
        return Err(CliFailure::Generator(Error::Unimplemented("embedded-spec".to_owned())));
    }

    let generate = &config.generate;
    if !generate.models && !generate.std_http_server && !generate.client && !generate.server_urls {
        return Err(CliFailure::NoArtifacts {
            config: cli.config_file.clone(),
        });
    }

    let output = cli.output_file.clone().or_else(|| {
        return config.output.clone();
    });
    let output = output.ok_or(CliFailure::NoOutput)?;

    let package = oapi_codegen::generate_package(&cli.spec_file, &config, &output)?;
    // Both scans read the generated code as text, and splitting it across files
    // must not change what either one concludes, so both read every file.
    let code = package.combined_source();
    if console::is_effectively_empty(&code) {
        let stats = spec_stats(&cli.spec_file, &config)?;
        return Err(CliFailure::EmptyOutput {
            spec: cli.spec_file.clone(),
            stats,
            generate: config.generate.clone(),
        });
    }

    if cli.check {
        // `--check` compares and writes nothing, so it reports no dependencies
        // either. A comparison adds no crate to the manifest.
        match oapi_codegen::check_package(&output, &package)? {
            PackageDrift::None => {
                console::report_check_passed(&output);
                return Ok(());
            }
            PackageDrift::Absent(path) => {
                return Err(CliFailure::Drift {
                    path,
                    kind: console::DriftKind::Absent,
                });
            }
            PackageDrift::Differs(path) => {
                return Err(CliFailure::Drift {
                    path,
                    kind: console::DriftKind::Differs,
                });
            }
            PackageDrift::Stale(path) => {
                return Err(CliFailure::Drift {
                    path,
                    kind: console::DriftKind::Stale,
                });
            }
        }
    }

    oapi_codegen::write_package(&output, &package)?;
    console::report_wrote(&output, package.file_count());

    let manifest = nearest_manifest(&output);
    let deps = oapi_codegen::deps::required_dependencies(&code);
    console::report_dependencies(&deps);
    if !deps.is_empty() && should_install_deps(cli.install_deps) {
        install_dependencies(&deps, manifest.as_deref());
    }
    return Ok(());
}

/// The nearest `Cargo.toml` at or above the output file's directory — the
/// package the generated code belongs to, used as the `cargo add` target.
///
/// This only locates the file. It does not parse it. `cargo add` interprets it
/// (and reports a clear error itself if it is a virtual workspace manifest).
fn nearest_manifest(output: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(output).unwrap_or_else(|_| return output.to_path_buf());
    let mut directory = canonical.parent();
    while let Some(current) = directory {
        let candidate = current.join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        directory = current.parent();
    }
    return None;
}

/// Decide whether to run `cargo add`: unconditionally when `--install-deps` is
/// set, otherwise by prompting an interactive terminal. A non-interactive run
/// without the flag only prints the list.
fn should_install_deps(flag: bool) -> bool {
    if flag {
        return true;
    }
    if std::io::stdin().is_terminal() {
        return console::prompt_install_dependencies();
    }
    return false;
}

/// Run `cargo add` for each dependency, targeting the package whose `Cargo.toml`
/// is nearest the output (or the current directory's when none was found).
/// `cargo add` merges with any existing declaration, so a crate already present
/// is updated in place rather than duplicated. A failure is reported as a
/// warning rather than aborting: the output file is already written, so
/// dependency installation is a best-effort convenience.
fn install_dependencies(deps: &[oapi_codegen::deps::Dependency], manifest: Option<&Path>) {
    for dep in deps {
        console::report_installing(dep);
        let mut command = Command::new("cargo");
        command.args(dep.cargo_add_args());
        if let Some(path) = manifest {
            command.arg("--manifest-path").arg(path);
        }
        match command.status() {
            Ok(status) if status.success() => {}
            Ok(status) => console::report_install_failed(dep, &format!("cargo exited with {status}")),
            Err(error) => console::report_install_failed(dep, &error.to_string()),
        }
    }
}

/// Compute a lightweight summary of the spec for empty-output reporting,
/// applying the same filters the generator used.
fn spec_stats(spec_path: &Path, config: &Config) -> Result<SpecStats> {
    let mut spec = oapi_codegen::loader::Spec::load(spec_path)?;
    spec.apply_filters(&config.output_options);
    return Ok(SpecStats {
        schemas: spec.schemas().len(),
        paths: spec.paths().paths.len(),
        servers: spec.servers().len(),
    });
}
