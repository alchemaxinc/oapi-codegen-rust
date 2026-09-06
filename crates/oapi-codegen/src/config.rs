//! Generator configuration, compatible with `oapi-codegen`'s YAML configuration files.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::diagnostic::Warning;
use crate::diagnostic::pointer;
use crate::diagnostic::report_warnings;
use crate::error::Error;
use crate::error::Result;

/// A generator configuration, mirroring the keys used by `oapi-codegen`.
///
/// Loading reports warnings for unknown keys and ignores them for compatibility.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    /// Target module/package name (informational for the Rust generator).
    pub package: Option<String>,
    /// Output file path for generated code, when configured.
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

/// The set of artifacts a configuration requests.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
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

/// Config key of the [`Config::output_options`] section, as written in a configuration
/// file. Must match the `kebab-case` serde name. Guarded by a deserialization
/// test.
pub(crate) const OUTPUT_OPTIONS_KEY: &str = "output-options";

/// Config key of [`OutputOptions::response_type_suffix`], as written in a configuration
/// file. Must match the `kebab-case` serde name. Guarded by a deserialization
/// test.
pub(crate) const RESPONSE_TYPE_SUFFIX_KEY: &str = "response-type-suffix";

/// Seed for a response enum's name suffix when
/// [`OutputOptions::response_type_suffix`] is unset.
/// The `to_ident(_, Pascal)` call turns it into the `Response` that terminates every
/// default `<Op>Response` enum.
pub(crate) const DEFAULT_RESPONSE_SUFFIX: &str = "response";

/// Config key of [`OutputOptions::type_name_suffix`], as written in a config
/// file. Must match the `kebab-case` serde name; guarded by a deserialization
/// test.
pub(crate) const TYPE_NAME_SUFFIX_KEY: &str = "type-name-suffix";

/// Output tuning options.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
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
    /// models are not generated. Filtering runs before pruning. If an excluded
    /// schema is still referenced by a retained operation or schema, generation
    /// can fail or emit a reference to a type that is not declared.
    #[serde(default)]
    pub exclude_schemas: Vec<String>,
    /// Suffix appended to a per-operation response enum's name (default
    /// `Response`). Set this to resolve a clash between a generated
    /// `<Op>Response` enum and a component schema of the same name, mirroring
    /// `oapi-codegen`'s `response-type-suffix`.
    #[serde(default)]
    pub response_type_suffix: Option<String>,
    /// Suffix added to the second of two schema names that collapse onto one
    /// Rust identifier. For example, `foo-bar` and `fooBar` both become `FooBar`.
    ///
    /// The default is unset. An unset suffix makes such a collision an error,
    /// because the generator will not pick a name for one of two distinct
    /// schemas. Use `x-rust-name` on the colliding schema first. That extension
    /// marks one schema and records the name the author wants. This option is
    /// for specs with many mechanical collisions, where one annotation per
    /// schema costs too much.
    ///
    /// A set suffix must hold at least one letter or digit. Casing removes
    /// punctuation, so a suffix such as `-` leaves the type name unchanged and
    /// cannot resolve a collision. See [`crate::lower::type_renames`].
    #[serde(default)]
    pub type_name_suffix: Option<String>,
}

impl Config {
    /// Load and parse a configuration file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| {
            return Error::ReadConfig {
                path: path.display().to_string(),
                source,
            };
        })?;
        let value: serde_yaml::Value = serde_yaml::from_str(&text).map_err(|source| {
            return Error::ParseConfig {
                path: path.display().to_string(),
                source,
            };
        })?;
        let warnings = configuration_warnings(&value).map_err(|source| {
            return Error::ParseConfig {
                path: path.display().to_string(),
                source,
            };
        })?;
        report_warnings(&path.display().to_string(), &warnings);
        let config: Config = serde_yaml::from_str(&text).map_err(|source| {
            return Error::ParseConfig {
                path: path.display().to_string(),
                source,
            };
        })?;
        return Ok(config);
    }
}

fn configuration_warnings(value: &serde_yaml::Value) -> serde_yaml::Result<Vec<Warning>> {
    let shape = serde_yaml::to_value(Config::default())?;
    let mut warnings = Vec::new();
    collect_unknown_keys(value, &shape, "", &mut warnings);
    return Ok(warnings);
}

fn collect_unknown_keys(
    value: &serde_yaml::Value,
    shape: &serde_yaml::Value,
    parent: &str,
    warnings: &mut Vec<Warning>,
) {
    let (Some(mapping), Some(fields)) = (value.as_mapping(), shape.as_mapping()) else {
        return;
    };
    // Empty default mappings contain user-defined keys.
    if fields.is_empty() {
        return;
    }
    for (key, value) in mapping {
        let Some(name) = key.as_str() else {
            continue;
        };
        let path = pointer(parent, name);
        if let Some(field) = fields.get(key) {
            collect_unknown_keys(value, field, &path, warnings);
        } else {
            warnings.push(Warning::new(path, "unknown configuration key is ignored"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_configuration_keys_warn() -> serde_yaml::Result<()> {
        for (yaml, paths) in [
            ("packge: demo", vec!["/packge"]),
            ("generate: {modles: true}", vec!["/generate/modles"]),
            ("output-options: {skip-prun: true}", vec!["/output-options/skip-prun"]),
            ("a~/b: secret", vec!["/a~0~1b"]),
            ("generate: {'~1/': true}", vec!["/generate/~01~1"]),
            (
                "packge: demo\ngenerate: {modles: true}\noutput-options: {skip-prun: true}",
                vec!["/packge", "/generate/modles", "/output-options/skip-prun"],
            ),
        ] {
            let value = serde_yaml::from_str(yaml)?;
            let expected: Vec<_> = paths
                .into_iter()
                .map(|path| {
                    return Warning::new(path, "unknown configuration key is ignored");
                })
                .collect();
            assert_eq!(configuration_warnings(&value)?, expected, "{yaml}");
            assert!(serde_yaml::from_value::<Config>(value).is_ok(), "{yaml}");
        }
        return Ok(());
    }

    #[test]
    fn known_configuration_keys_do_not_warn() -> serde_yaml::Result<()> {
        for yaml in [
            "{}",
            "package: demo\noutput: generated.rs",
            "generate: {models: true, std-http-server: true, client: true, embedded-spec: false, server-urls: true}",
            "import-mapping: {'./other.yaml': other, 'a~/b': module}",
            "output-options: {include-tags: [pets], response-type-suffix: Resp, type-name-suffix: Alt}",
        ] {
            let value = serde_yaml::from_str(yaml)?;
            assert!(configuration_warnings(&value)?.is_empty(), "{yaml}");
            assert!(serde_yaml::from_value::<Config>(value).is_ok(), "{yaml}");
        }
        let defaults = serde_yaml::to_value(Config::default())?;
        assert!(configuration_warnings(&defaults)?.is_empty());
        return Ok(());
    }

    #[test]
    fn invalid_configuration_values_remain_deserialization_errors() -> serde_yaml::Result<()> {
        for yaml in ["null", "generate: null", "output-options: null", "import-mapping: null"] {
            let value = serde_yaml::from_str(yaml)?;
            assert!(configuration_warnings(&value)?.is_empty(), "{yaml}");
        }
        for yaml in [
            "false",
            "[]",
            "generate: wrong",
            "generate: {models: wrong}",
            "output-options: []",
            "output-options: {include-tags: false}",
            "import-mapping: {other: []}",
        ] {
            let value = serde_yaml::from_str(yaml)?;
            assert!(configuration_warnings(&value)?.is_empty(), "{yaml}");
            assert!(serde_yaml::from_value::<Config>(value).is_err(), "{yaml}");
        }
        let value = serde_yaml::from_str("generate: {modles: true, models: wrong}")?;
        assert_eq!(
            configuration_warnings(&value)?,
            vec![Warning::new("/generate/modles", "unknown configuration key is ignored")]
        );
        assert!(serde_yaml::from_value::<Config>(value).is_err());
        return Ok(());
    }

    #[test]
    fn config_keys_match_serde_names() {
        let yaml =
            format!("{OUTPUT_OPTIONS_KEY}:\n  {RESPONSE_TYPE_SUFFIX_KEY}: Resp\n  {TYPE_NAME_SUFFIX_KEY}: Alt\n",);
        let config: Config = serde_yaml::from_str(&yaml).expect("config parses");
        assert_eq!(
            config.output_options.response_type_suffix.as_deref(),
            Some("Resp"),
            "OUTPUT_OPTIONS_KEY/RESPONSE_TYPE_SUFFIX_KEY drifted from the serde field names",
        );
        assert_eq!(
            config.output_options.type_name_suffix.as_deref(),
            Some("Alt"),
            "TYPE_NAME_SUFFIX_KEY drifted from the serde field name",
        );
    }

    #[test]
    fn type_name_suffix_defaults_to_unset() {
        // An unset suffix must mean "stop on a collision", so the default is
        // `None` and not a fallback string.
        let config: Config = serde_yaml::from_str("package: demo\n").expect("config parses");
        assert_eq!(config.output_options.type_name_suffix, None);
    }

    #[test]
    fn default_response_suffix_pascalizes_to_response() {
        use crate::naming::Case;
        use crate::naming::to_ident;

        assert_eq!(to_ident(DEFAULT_RESPONSE_SUFFIX, Case::Pascal).logical(), "Response");
    }
}
