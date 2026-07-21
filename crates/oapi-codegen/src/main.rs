//! Command-line entry point for the `oapi-codegen` generator.
#![allow(
    clippy::print_stderr,
    reason = "this binary reports status and guided errors to stderr"
)]

mod console;

use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use oapi_codegen::Config;
use oapi_codegen::Error;
use oapi_codegen::Result;
use oapi_codegen::cli::Cli;
use oapi_codegen::config::Generate;

use crate::console::SpecStats;

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

    let code = oapi_codegen::generate(&cli.spec_file, &config)?;
    if console::is_effectively_empty(&code) {
        let stats = spec_stats(&cli.spec_file, &config)?;
        return Err(CliFailure::EmptyOutput {
            spec: cli.spec_file.clone(),
            stats,
            generate: config.generate.clone(),
        });
    }

    oapi_codegen::write_output(&output, &code)?;
    console::report_wrote(&output);
    return Ok(());
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
