//! Error types for the generator.

/// Errors that can occur while loading a spec or generating code.
#[derive(Debug)]
pub enum Error {
    /// The spec file could not be read from disk.
    ReadSpec {
        /// Path that could not be read.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// The spec file could not be parsed as OpenAPI YAML/JSON.
    ParseSpec {
        /// Path that could not be parsed.
        path: String,
        /// Underlying parse error.
        source: serde_yaml::Error,
    },

    /// The config file could not be read from disk.
    ReadConfig {
        /// Path that could not be read.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// The config file could not be parsed as YAML.
    ParseConfig {
        /// Path that could not be parsed.
        path: String,
        /// Underlying parse error.
        source: serde_yaml::Error,
    },

    /// The requested generation mode is not implemented yet.
    Unimplemented(String),

    /// Writing the generated output failed.
    WriteOutput {
        /// Path that could not be written.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// A `$ref` pointed at something that could not be resolved.
    UnresolvedRef(String),

    /// A `$ref` used a form the generator does not support yet.
    UnsupportedRef {
        /// The offending reference string.
        reference: String,
        /// Why it is unsupported.
        reason: String,
    },

    /// A schema combined keywords in a way the generator cannot represent.
    UnsupportedSchema {
        /// Dotted path to the schema for diagnostics.
        path: String,
        /// Why it is unsupported.
        reason: String,
    },

    /// An operation used a feature the server generator does not support yet.
    UnsupportedOperation {
        /// HTTP method of the offending operation.
        method: String,
        /// Templated request path of the offending operation.
        path: String,
        /// Why it is unsupported.
        reason: String,
    },

    /// The generated token stream was not valid Rust (internal bug).
    InvalidGeneratedCode {
        /// Underlying syn parse error.
        source: syn::Error,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ReadSpec { path, source } => {
                return write!(f, "failed to read spec file `{path}`: {source}");
            }
            Error::ParseSpec { path, source } => {
                return write!(f, "failed to parse spec file `{path}`: {source}");
            }
            Error::ReadConfig { path, source } => {
                return write!(f, "failed to read config file `{path}`: {source}");
            }
            Error::ParseConfig { path, source } => {
                return write!(f, "failed to parse config file `{path}`: {source}");
            }
            Error::Unimplemented(mode) => {
                return write!(f, "{mode} generation is not implemented yet");
            }
            Error::WriteOutput { path, source } => {
                return write!(f, "failed to write output `{path}`: {source}");
            }
            Error::UnresolvedRef(reference) => {
                return write!(f, "unresolved reference `{reference}`");
            }
            Error::UnsupportedRef { reference, reason } => {
                return write!(f, "unsupported reference `{reference}`: {reason}");
            }
            Error::UnsupportedSchema { path, reason } => {
                return write!(f, "unsupported schema at `{path}`: {reason}");
            }
            Error::UnsupportedOperation { method, path, reason } => {
                return write!(f, "unsupported operation `{method} {path}`: {reason}");
            }
            Error::InvalidGeneratedCode { source } => {
                return write!(f, "generated code was not valid Rust: {source}");
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::ReadSpec { source, .. } => return Some(source),
            Error::ParseSpec { source, .. } => return Some(source),
            Error::ReadConfig { source, .. } => return Some(source),
            Error::ParseConfig { source, .. } => return Some(source),
            Error::WriteOutput { source, .. } => return Some(source),
            Error::InvalidGeneratedCode { source } => return Some(source),
            Error::Unimplemented(_)
            | Error::UnresolvedRef(_)
            | Error::UnsupportedRef { .. }
            | Error::UnsupportedSchema { .. }
            | Error::UnsupportedOperation { .. } => return None,
        }
    }
}

/// Convenience alias for results in this crate.
pub type Result<T> = std::result::Result<T, Error>;
