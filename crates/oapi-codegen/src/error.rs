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

    /// A referenced external file could not be read from disk.
    ReadRefFile {
        /// The referenced file, as written in the `$ref`.
        file: String,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// A referenced external file could not be parsed as OpenAPI YAML/JSON.
    ParseRefFile {
        /// The referenced file, as written in the `$ref`.
        file: String,
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

    /// A parameter declared `in: path` has no matching `{placeholder}` in the
    /// operation's path template. An OpenAPI path parameter must appear in the
    /// path, and lowering it from the template would otherwise silently drop it
    /// from the generated signature.
    InvalidPathParameter {
        /// HTTP method of the offending operation.
        method: String,
        /// Templated request path of the offending operation.
        path: String,
        /// The declared path-parameter name with no matching placeholder.
        name: String,
    },

    /// A `{placeholder}` in the operation's path template has no matching
    /// parameter declared `in: path`. The generator cannot know the parameter's
    /// type, so rather than silently assume `String` it requires the parameter
    /// to be declared (matching the OpenAPI requirement that every path template
    /// variable have a corresponding path parameter).
    UndeclaredPathParameter {
        /// HTTP method of the offending operation.
        method: String,
        /// Templated request path of the offending operation.
        path: String,
        /// The template placeholder name with no declared parameter.
        name: String,
    },

    /// A generated per-operation type name collided with a component-model name
    /// emitted in the same file.
    TypeNameCollision {
        /// The clashing Rust identifier.
        name: String,
        /// The generated artifact that clashed (e.g. `response enum`).
        artifact: String,
        /// How to resolve the clash.
        hint: String,
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
            Error::ReadRefFile { file, source } => {
                return write!(f, "failed to read referenced file `{file}`: {source}");
            }
            Error::ParseRefFile { file, source } => {
                return write!(f, "failed to parse referenced file `{file}`: {source}");
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
            Error::InvalidPathParameter { method, path, name } => {
                return write!(
                    f,
                    "path parameter `{name}` on `{method} {path}` is declared `in: path` but the path template has no `{{{name}}}` placeholder"
                );
            }
            Error::UndeclaredPathParameter { method, path, name } => {
                return write!(
                    f,
                    "operation `{method} {path}` has a `{{{name}}}` placeholder in its path but no parameter named `{name}` is declared `in: path`"
                );
            }
            Error::TypeNameCollision { name, artifact, hint } => {
                return write!(
                    f,
                    "generated {artifact} `{name}` collides with a component schema of the same name; {hint}"
                );
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
            Error::ReadRefFile { source, .. } => return Some(source),
            Error::ParseRefFile { source, .. } => return Some(source),
            Error::ReadConfig { source, .. } => return Some(source),
            Error::ParseConfig { source, .. } => return Some(source),
            Error::WriteOutput { source, .. } => return Some(source),
            Error::InvalidGeneratedCode { source } => return Some(source),
            Error::Unimplemented(_)
            | Error::UnresolvedRef(_)
            | Error::UnsupportedRef { .. }
            | Error::UnsupportedSchema { .. }
            | Error::TypeNameCollision { .. }
            | Error::InvalidPathParameter { .. }
            | Error::UndeclaredPathParameter { .. }
            | Error::UnsupportedOperation { .. } => return None,
        }
    }
}

/// Convenience alias for results in this crate.
pub type Result<T> = std::result::Result<T, Error>;
