//! Definition of the command-line interface.
//!
//! [`Cli`] is the source of truth for the CLI.
//! The binary parses this definition.
//! The `cli_docs` test renders this definition as Markdown.
//! This keeps `docs/cli.md` in sync with the interface.

use std::path::PathBuf;

use clap::Parser;

/// Extra `--help` text with examples.
pub const EXAMPLES: &str = "\
Examples:
  # Write generated code. Select artifacts in the configuration file:
  oapi-codegen --config-file oapi-codegen.yaml --output-file src/api.rs api.yaml

  # You can also set output with the `output:` configuration key:
  oapi-codegen --config-file oapi-codegen.yaml api.yaml

You must provide a configuration file.
The configuration file must enable at least one artifact.
You must set output with --output-file or the `output:` configuration key:
  # oapi-codegen.yaml
  output: src/api.rs
  generate:
    models: true
    std-http-server: true
    client: true";

/// Generate Rust code from an OpenAPI 3 specification.
#[derive(Debug, Parser)]
#[command(name = "oapi-codegen", version, about, after_long_help = EXAMPLES)]
pub struct Cli {
    /// Path to the OpenAPI 3 specification (YAML or JSON).
    pub spec_file: PathBuf,

    /// Path to an `oapi-codegen` YAML configuration file (required).
    #[arg(short = 'c', long)]
    pub config_file: PathBuf,

    /// Output file path (overrides configuration `output:`).
    /// Required unless the configuration sets `output:`.
    #[arg(short = 'o', long)]
    pub output_file: Option<PathBuf>,

    /// After the write, run `cargo add` for each required crate.
    ///
    /// If set, this runs with no prompt.
    /// If stdin is interactive and the flag is not set, the CLI asks first.
    /// If stdin is not interactive and the flag is not set, the CLI prints only the list.
    /// `cargo add` targets the package whose `Cargo.toml` is nearest output.
    /// It merges with an existing declaration.
    /// A crate that already exists is updated in place.
    #[arg(long)]
    pub install_deps: bool,
}
