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

    /// Inline schema nesting exceeded the depth the generator will lower,
    /// guarding against stack exhaustion on hostile or pathological specs.
    SchemaDepthExceeded {
        /// Schema name / lowering hint identifying the offending inline schema.
        path: String,
        /// The maximum supported inline nesting depth.
        limit: usize,
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
        /// The generated artifact that clashed (for example `response enum`).
        artifact: String,
        /// How to resolve the clash. Rendered by the console as a hint, and not
        /// by `Display`, so the console does not print it twice.
        hint: String,
    },

    /// Two component schema names collapsed onto one Rust identifier.
    ///
    /// The generator will not choose which schema keeps the plain name, because
    /// that choice belongs to the spec author.
    SchemaNameCollision {
        /// The Rust identifier that both schemas produce.
        ident: String,
        /// The schema that claimed the identifier first, in document order.
        first: String,
        /// The schema that collided with `first`.
        second: String,
        /// How to resolve the clash. Rendered by the console as a hint, and not
        /// by `Display`, so the console does not print it twice.
        hint: String,
    },

    /// `output-options.type-name-suffix` holds no identifier characters.
    ///
    /// Casing drops punctuation and separators, so a suffix such as `-` or `_`
    /// adds nothing to a type name. The generator cannot resolve a collision with
    /// such a suffix, because the second name stays the same as the first.
    InvalidTypeNameSuffix {
        /// The configured suffix, as written in the config.
        suffix: String,
        /// How to resolve the problem. Rendered by the console as a hint, and not
        /// by `Display`, so the console does not print it twice.
        hint: String,
    },

    /// The generated token stream was not valid Rust (internal bug).
    InvalidGeneratedCode {
        /// Underlying syn parse error.
        source: syn::Error,
    },

    /// One pass found several independent semantic problems.
    ///
    /// This variant holds two or more problems. One problem returns as itself, so
    /// a caller can match that variant. See
    /// [`crate::lower::validate::Diagnostics::into_result`]. This variant never
    /// nests, because the collector holds leaf errors only.
    Validation {
        /// The problems, in discovery order.
        problems: Vec<Error>,
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
            Error::SchemaDepthExceeded { path, limit } => {
                return write!(f, "schema at `{path}` nests deeper than the supported limit of {limit}");
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
            // The remedy is a hint, which the console prints under the message.
            // `Display` therefore states the problem only.
            Error::TypeNameCollision { name, artifact, .. } => {
                return write!(
                    f,
                    "generated {artifact} `{name}` collides with a component schema of the same name"
                );
            }
            Error::SchemaNameCollision {
                ident, first, second, ..
            } => {
                return write!(
                    f,
                    "component schemas `{first}` and `{second}` both produce the Rust type name `{ident}`"
                );
            }
            Error::InvalidTypeNameSuffix { suffix, .. } => {
                return write!(
                    f,
                    "`type-name-suffix` is set to `{suffix}`, which contributes no characters to a Rust type name"
                );
            }
            Error::InvalidGeneratedCode { source } => {
                return write!(f, "generated code was not valid Rust: {source}");
            }
            Error::Validation { problems } => {
                write!(f, "found {} problems in the spec:", problems.len())?;
                for problem in problems {
                    write!(f, "\n  - {problem}")?;
                }
                return Ok(());
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
            // `Validation` holds problems at the same level and wraps no cause.
            // It has no single `source`. `Display` shows the problems instead.
            Error::Validation { .. }
            | Error::Unimplemented(_)
            | Error::UnresolvedRef(_)
            | Error::UnsupportedRef { .. }
            | Error::UnsupportedSchema { .. }
            | Error::SchemaDepthExceeded { .. }
            | Error::TypeNameCollision { .. }
            | Error::SchemaNameCollision { .. }
            | Error::InvalidTypeNameSuffix { .. }
            | Error::InvalidPathParameter { .. }
            | Error::UndeclaredPathParameter { .. }
            | Error::UnsupportedOperation { .. } => return None,
        }
    }
}

/// Convenience alias for results in this crate.
pub type Result<T> = std::result::Result<T, Error>;
