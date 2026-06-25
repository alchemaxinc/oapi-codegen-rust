//! Command-line entry point for the `oapi-codegen` generator.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use oapi_codegen::Config;
use oapi_codegen::Error;
use oapi_codegen::Result;
use oapi_codegen::config::Generate;

/// Generate idiomatic Rust from an OpenAPI 3 specification.
#[derive(Debug, Parser)]
#[command(name = "oapi-codegen", version, about)]
struct Cli {
    /// Path to the OpenAPI 3 specification (YAML or JSON).
    spec: PathBuf,

    /// Path to an `oapi-codegen` YAML config file.
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Output file path (overrides the config `output`). Prints to stdout if unset.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => {
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    }
}

/// Run the generator according to the parsed CLI arguments.
fn run(cli: &Cli) -> Result<()> {
    let config = match &cli.config {
        Some(path) => Config::load(path)?,
        None => Config {
            generate: Generate {
                models: true,
                ..Default::default()
            },
            ..Default::default()
        },
    };

    if config.generate.embedded_spec {
        return Err(Error::Unimplemented("embedded-spec".to_owned()));
    }
    if !config.generate.models && !config.generate.std_http_server {
        eprintln!("nothing to generate: enable `models` or `std-http-server` in the config");
        return Ok(());
    }

    let output = cli.output.clone().or_else(|| {
        return config.output.clone();
    });
    match output {
        Some(path) => {
            oapi_codegen::generate_to_file(&cli.spec, &config, &path)?;
            eprintln!("wrote {}", path.display());
            return Ok(());
        }
        None => {
            let code = oapi_codegen::generate(&cli.spec, &config)?;
            print!("{code}");
            return Ok(());
        }
    }
}
