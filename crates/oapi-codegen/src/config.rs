//! Generator configuration, compatible with `oapi-codegen`'s YAML config files.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

use crate::error::Error;
use crate::error::Result;

/// A generator configuration, mirroring the keys used by `oapi-codegen`.
///
/// Unknown keys are ignored so that existing `oapi-codegen` configs can be used
/// as-is; only the subset relevant to this tool is interpreted.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    /// Target module/package name (informational for the Rust generator).
    pub package: Option<String>,
    /// Output file path the generated code should be written to.
    pub output: Option<PathBuf>,
    /// Which artifacts to generate.
    #[serde(default)]
    pub generate: Generate,
    /// Output tuning options.
    #[serde(default)]
    pub output_options: OutputOptions,
    /// Mapping of referenced spec files to external modules (server generation).
    #[serde(default)]
    pub import_mapping: BTreeMap<String, String>,
}

/// The set of artifacts a config requests.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Generate {
    /// Generate data models (structs/enums) from component schemas.
    #[serde(default)]
    pub models: bool,
    /// Generate an axum server interface from the spec's paths.
    #[serde(default)]
    pub std_http_server: bool,
    /// Generate a blocking `reqwest` client from the spec's paths.
    #[serde(default)]
    pub client: bool,
    /// Embed the spec into the generated code (not yet implemented).
    #[serde(default)]
    pub embedded_spec: bool,
    /// Emit constants and builder functions for the spec's `servers` URLs.
    #[serde(default)]
    pub server_urls: bool,
}

/// Output tuning options.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OutputOptions {
    /// Keep schemas that are not referenced (no pruning).
    #[serde(default)]
    pub skip_prune: bool,
    /// Only generate operations tagged with one of these tags (empty = all).
    #[serde(default)]
    pub include_tags: Vec<String>,
    /// Skip operations tagged with any of these tags.
    #[serde(default)]
    pub exclude_tags: Vec<String>,
    /// Only generate operations whose `operationId` is one of these
    /// (empty = all).
    #[serde(default)]
    pub include_operation_ids: Vec<String>,
    /// Skip operations whose `operationId` is one of these.
    #[serde(default)]
    pub exclude_operation_ids: Vec<String>,
    /// Remove these component schemas from the spec before lowering, so their
    /// models are not generated. Filtering runs before pruning; if an excluded
    /// schema is still referenced by a retained operation or schema, generation
    /// may fail or emit a reference to a type that is not declared.
    #[serde(default)]
    pub exclude_schemas: Vec<String>,
    /// Suffix appended to a per-operation response enum's name (default
    /// `Response`). Set this to resolve a clash between a generated
    /// `<Op>Response` enum and a component schema of the same name, mirroring
    /// `oapi-codegen`'s `response-type-suffix`.
    #[serde(default)]
    pub response_type_suffix: Option<String>,
}

impl Config {
    /// Load and parse a config file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| {
            return Error::ReadConfig {
                path: path.display().to_string(),
                source,
            };
        })?;
        let config: Config = serde_yaml::from_str(&text).map_err(|source| {
            return Error::ParseConfig {
                path: path.display().to_string(),
                source,
            };
        })?;
        return Ok(config);
    }
}
