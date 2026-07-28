//! Command-line interface definition.
//!
//! [`Cli`] is the single source of truth for the CLI.
//! The binary parses it.
//! The `cli_docs` test renders it to Markdown.
//! This keeps `docs/cli.md` in sync with the real interface.

use std::path::PathBuf;

use clap::Parser;

/// Extra `--help` text with examples.
pub const EXAMPLES: &str = "\
Examples:
  # Write generated code. Select artifacts in the config file:
  oapi-codegen --config-file oapi-codegen.yaml --output-file src/api.rs api.yaml

  # You can also set output with the config `output:` key:
  oapi-codegen --config-file oapi-codegen.yaml api.yaml

You must provide a config file.
The config file must enable at least one artifact.
You must set output with --output-file or config `output:`:
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

    /// Path to an `oapi-codegen` YAML config file (required).
    #[arg(short = 'c', long)]
    pub config_file: PathBuf,

    /// Output file path (overrides config `output:`).
    /// Required unless the config sets `output:`.
    #[arg(short = 'o', long)]
    pub output_file: Option<PathBuf>,

    /// After write, run `cargo add` for each required crate.
    ///
    /// If set, this runs with no prompt.
    /// If not set and stdin is interactive, you are asked first.
    /// If not set and stdin is not interactive, the run prints only the list.
    /// `cargo add` targets the package whose `Cargo.toml` is nearest output.
    /// It merges with an existing declaration.
    /// A crate that already exists is updated in place.
    #[arg(long)]
    pub install_deps: bool,
}
