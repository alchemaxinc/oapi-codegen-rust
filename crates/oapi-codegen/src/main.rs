//! Command-line entry point for the `oapi-codegen` generator.
#![allow(
    clippy::print_stderr,
    reason = "this binary reports status and guided errors to stderr"
)]

mod diag;

use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use oapi_codegen::Config;
use oapi_codegen::Error;
use oapi_codegen::Result;
use oapi_codegen::config::Generate;

use crate::diag::SpecStats;

/// Extra `--help` text with worked examples.
const EXAMPLES: &str = "\
Examples:
  # Write generated code, selecting artifacts in the config file:
  oapi-codegen api.yaml --config oapi-codegen.yaml --output src/api.rs

  # The output path may instead come from the config's `output:` key:
  oapi-codegen api.yaml --config oapi-codegen.yaml

A config file is required, must enable at least one artifact, and an output
path must be given via --output or the config's `output:` key:
  # oapi-codegen.yaml
  output: src/api.rs
  generate:
    models: true
    std-http-server: true
    client: true";

/// Generate idiomatic Rust from an OpenAPI 3 specification.
#[derive(Debug, Parser)]
#[command(name = "oapi-codegen", version, about, after_long_help = EXAMPLES)]
struct Cli {
    /// Path to the OpenAPI 3 specification (YAML or JSON).
    spec: PathBuf,

    /// Path to an `oapi-codegen` YAML config file (required).
    #[arg(short, long)]
    config: PathBuf,

    /// Output file path (overrides the config `output`). Required unless the
    /// config sets `output`.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

/// A failure that has enough context to be reported to the user.
enum CliFailure {
    /// A generator/loader error occurred.
    Generator(Error),
    /// The config parsed but enabled no artifacts.
    NoArtifacts {
        /// Path to the offending config file.
        config: PathBuf,
    },
    /// No output destination was given on the CLI or in the config.
    NoOutput,
    /// Generation succeeded but produced no code.
    EmptyOutput {
        /// The spec that was processed.
        spec: PathBuf,
        /// Counts explaining why nothing was generated.
        stats: SpecStats,
        /// What the config requested.
        generate: Generate,
    },
}

impl CliFailure {
    /// Print a guided report for this failure to stderr.
    fn report(&self) {
        match self {
            CliFailure::Generator(err) => {
                diag::report_error(err);
            }
            CliFailure::NoArtifacts { config } => {
                diag::report_no_artifacts(config);
            }
            CliFailure::NoOutput => {
                diag::report_no_output();
            }
            CliFailure::EmptyOutput { spec, stats, generate } => {
                diag::report_empty_output(spec, stats, generate);
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
    let config = Config::load(&cli.config)?;

    if config.generate.embedded_spec {
        return Err(CliFailure::Generator(Error::Unimplemented("embedded-spec".to_owned())));
    }

    let generate = &config.generate;
    if !generate.models && !generate.std_http_server && !generate.client {
        return Err(CliFailure::NoArtifacts {
            config: cli.config.clone(),
        });
    }

    let output = cli.output.clone().or_else(|| {
        return config.output.clone();
    });
    let output = output.ok_or(CliFailure::NoOutput)?;

    let code = oapi_codegen::generate(&cli.spec, &config)?;
    if diag::is_effectively_empty(&code) {
        let stats = spec_stats(&cli.spec, &config)?;
        return Err(CliFailure::EmptyOutput {
            spec: cli.spec.clone(),
            stats,
            generate: config.generate.clone(),
        });
    }

    write_output_file(&code, &output)?;
    diag::report_wrote(&output);
    return Ok(());
}

/// Compute a lightweight summary of the spec for empty-output diagnostics,
/// applying the same filters the generator used.
fn spec_stats(spec_path: &Path, config: &Config) -> Result<SpecStats> {
    let mut spec = oapi_codegen::loader::Spec::load(spec_path)?;
    spec.apply_filters(&config.output_options);
    return Ok(SpecStats {
        schemas: spec.schemas().len(),
        paths: spec.paths().paths.len(),
    });
}

/// Write generated `code` to `path`, creating parent directories as needed.
fn write_output_file(code: &str, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|source| {
            return Error::WriteOutput {
                path: path.display().to_string(),
                source,
            };
        })?;
    }
    std::fs::write(path, code).map_err(|source| {
        return Error::WriteOutput {
            path: path.display().to_string(),
            source,
        };
    })?;
    return Ok(());
}
