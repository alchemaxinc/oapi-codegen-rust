//! The command-line interface definition.
//!
//! The [`Cli`] struct is the single source of truth for the CLI: the binary
//! parses it, and the `cli_docs` integration test renders it to Markdown so
//! `docs/cli.md` never drifts from the real interface.

use std::path::PathBuf;

use clap::Parser;

/// Extra `--help` text with worked examples.
pub const EXAMPLES: &str = "\
Examples:
  # Write generated code, selecting artifacts in the config file:
  oapi-codegen --config-file oapi-codegen.yaml --output-file src/api.rs api.yaml

  # The output path may instead come from the config's `output:` key:
  oapi-codegen --config-file oapi-codegen.yaml api.yaml

A config file is required, must enable at least one artifact, and an output
path must be given via --output-file or the config's `output:` key:
  # oapi-codegen.yaml
  output: src/api.rs
  generate:
    models: true
    std-http-server: true
    client: true";

/// Generate idiomatic Rust from an OpenAPI 3 specification.
#[derive(Debug, Parser)]
#[command(name = "oapi-codegen", version, about, after_long_help = EXAMPLES)]
pub struct Cli {
    /// Path to the OpenAPI 3 specification (YAML or JSON).
    pub spec_file: PathBuf,

    /// Path to an `oapi-codegen` YAML config file (required).
    #[arg(short = 'c', long)]
    pub config_file: PathBuf,

    /// Output file path (overrides the config `output:`). Required unless the
    /// config sets `output:`.
    #[arg(short = 'o', long)]
    pub output_file: Option<PathBuf>,

    /// After writing, run `cargo add` for each crate the generated code needs,
    /// without prompting. On an interactive terminal without this flag you are
    /// asked first; a non-interactive run only prints the dependency list.
    /// `cargo add` targets the current directory's package.
    #[arg(long)]
    pub install_deps: bool,
}
