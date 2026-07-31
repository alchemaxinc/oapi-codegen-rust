//! Error types for the generator.

/// Errors that can occur while loading a spec or generating code.
#[derive(Debug)]
pub enum Error {
    /// The spec file cannot be read from disk.
    ReadSpec {
        /// Path that cannot be read.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// The spec file cannot be parsed as OpenAPI YAML/JSON.
    ParseSpec {
        /// Path that cannot be parsed.
        path: String,
        /// Underlying parse error.
        source: serde_yaml::Error,
    },

    /// A referenced external file cannot be read from disk.
    ReadRefFile {
        /// The referenced file, as written in the `$ref`.
        file: String,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// A referenced external file cannot be parsed as OpenAPI YAML/JSON.
    ParseRefFile {
        /// The referenced file, as written in the `$ref`.
        file: String,
        /// Underlying parse error.
        source: serde_yaml::Error,
    },

    /// The configuration file cannot be read from disk.
    ReadConfig {
        /// Path that cannot be read.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// The configuration file cannot be parsed as YAML.
    ParseConfig {
        /// Path that cannot be parsed.
        path: String,
        /// Underlying parse error.
        source: serde_yaml::Error,
    },

    /// The requested generation mode is not implemented yet.
    Unimplemented(String),

    /// Writing the generated output failed.
    WriteOutput {
        /// Path that cannot be written.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// Reading the output file for a comparison failed.
    ///
    /// An absent file is not this error. `--check` reports an absent file as
    /// drift, because generation creates it. This covers a file that exists and
    /// that the process cannot read, such as a directory or a file with no read
    /// permission.
    ReadOutput {
        /// Path that cannot be read.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// The document declares an OpenAPI version the generator does not read.
    ///
    /// Only `3.0.x` is supported. A `3.1` document is rejected and not read as a
    /// 3.0 document, because the subsets overlap. A 3.1 document whose every
    /// construct happens to parse as 3.0 would otherwise generate quietly, and
    /// one 3.1-only construct in the same file would fail with a `serde` message
    /// that names neither the version nor the reason.
    UnsupportedSpecVersion {
        /// The document that declares it, as a path or as the `$ref` that
        /// reached it. Every parsed document is checked, so the message must
        /// name which one failed.
        document: String,
        /// The `openapi:` value the document declares.
        version: String,
        /// What the generator reads instead.
        hint: String,
    },

    /// The document declares a top-level key the generator cannot generate from.
    ///
    /// `webhooks:` is the case. It is a 3.1 key that carries operations, and a
    /// generator that ignores it emits no handler for any of them. Silence here
    /// reads as "the spec declares no such operation".
    UnsupportedSpecKey {
        /// The top-level key, as written in the document.
        key: String,
        /// Why the generator cannot generate from it.
        reason: String,
        /// What to do instead.
        hint: String,
    },

    /// A `$ref` pointed at something that cannot be resolved.
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

    /// A request or response body declares content, and no content type the
    /// generator can represent.
    ///
    /// This is not a bodyless body. A bodyless response declares no `content:`
    /// at all, and `204` is the common case. A body that declares
    /// `application/pdf` states that a payload exists, so emitting no field for
    /// it drops the payload with no message.
    ///
    /// Both directions report through this one variant, because both make the
    /// same statement about the same input. The remedy differs by direction, so
    /// the hint carries it.
    UnsupportedContentType {
        /// HTTP method of the offending operation.
        method: String,
        /// Templated request path of the offending operation.
        path: String,
        /// Which body it is, as a noun phrase for the message (`request body`,
        /// or a response named by its status code).
        location: String,
        /// The declared content types, in document order, comma separated.
        declared: String,
        hint: String,
    },

    /// A parameter declared `in: path` has no matching `{placeholder}` in the
    /// operation's path template. An OpenAPI path parameter must appear in the
    /// path, and lowering it from the template will otherwise silently drop it
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

    /// Two emitted items took one Rust type name, and at least one of them came
    /// from an inline schema that lowering hoisted to the crate root.
    ///
    /// Two component schemas that collapse onto one identifier are reported as
    /// [`Error::SchemaNameCollision`], which names both schemas. A hoisted inline
    /// schema has no name of its own, so this variant names the identifier only.
    DuplicateTypeName {
        /// The Rust type name that two emitted items take.
        name: String,
        /// How to resolve the clash. Rendered by the console as a hint, and not
        /// by `Display`, so the console does not print it twice.
        hint: String,
    },

    /// A per-operation type took the Rust type name of a second per-operation
    /// type, or of a generator interface.
    ///
    /// Every per-operation type name derives from the method name of its
    /// operation and a fixed suffix, so two of them clash when a configured suffix
    /// makes them equal, or when two method names differ only by a suffix that
    /// another artifact also adds. The same suffix can also give a per-operation
    /// type the fixed name of a requested interface, such as the `Api` trait.
    ///
    /// A clash with a model is reported as [`Error::TypeNameCollision`] instead,
    /// because the remedy names the schema and not an operation.
    OperationTypeCollision {
        /// The Rust type name that both items take.
        name: String,
        /// The item that claimed the name first, in document order, as a noun
        /// phrase. A per-operation type names its kind and its operation. A
        /// generator interface names what emits it and holds no operation, because
        /// the name is fixed and belongs to no operation.
        first: String,
        /// The item that collided with `first`, always a per-operation type, in the
        /// same form.
        second: String,
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

    /// Two operations collapsed onto one Rust method name.
    ///
    /// Every artifact of an operation derives from this one name, so the file
    /// holds a duplicate trait method, response enum, and handler, and the router
    /// points both routes at one handler. The generator will not choose which
    /// operation keeps the plain name, because that choice belongs to the spec
    /// author.
    OperationNameCollision {
        /// The Rust method name that both operations produce.
        ident: String,
        /// `method path` of the operation that claimed the name first, in
        /// document order.
        first: String,
        /// `method path` of the operation that collided with `first`.
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
            Error::ReadOutput { path, source } => {
                return write!(f, "failed to read output `{path}`: {source}");
            }
            // The remedy is a hint, which the console prints under the message.
            // `Display` therefore states the problem only.
            Error::UnsupportedSpecVersion { document, version, .. } => {
                return write!(f, "`{document}` declares `openapi: {version}`, which is not supported");
            }
            Error::UnsupportedSpecKey { key, reason, .. } => {
                return write!(f, "the document declares `{key}:`, which {reason}");
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
            Error::UnsupportedContentType {
                method,
                path,
                location,
                declared,
                ..
            } => {
                return write!(
                    f,
                    "the {location} of `{method} {path}` declares only content types the generator cannot represent: {declared}"
                );
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
            Error::DuplicateTypeName { name, .. } => {
                return write!(f, "two generated items both take the Rust type name `{name}`");
            }
            Error::OperationTypeCollision {
                name, first, second, ..
            } => {
                return write!(f, "{first} and {second} both take the Rust type name `{name}`");
            }
            Error::SchemaNameCollision {
                ident, first, second, ..
            } => {
                return write!(
                    f,
                    "component schemas `{first}` and `{second}` both produce the Rust type name `{ident}`"
                );
            }
            Error::OperationNameCollision {
                ident, first, second, ..
            } => {
                return write!(
                    f,
                    "operations `{first}` and `{second}` both produce the Rust method name `{ident}`"
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
            Error::ReadOutput { source, .. } => return Some(source),
            Error::InvalidGeneratedCode { source } => return Some(source),
            // `Validation` holds problems at the same level and wraps no cause.
            // It has no single `source`. `Display` shows the problems instead.
            Error::Validation { .. }
            | Error::Unimplemented(_)
            | Error::UnsupportedSpecVersion { .. }
            | Error::UnsupportedSpecKey { .. }
            | Error::UnsupportedContentType { .. }
            | Error::UnresolvedRef(_)
            | Error::UnsupportedRef { .. }
            | Error::UnsupportedSchema { .. }
            | Error::SchemaDepthExceeded { .. }
            | Error::TypeNameCollision { .. }
            | Error::DuplicateTypeName { .. }
            | Error::OperationTypeCollision { .. }
            | Error::SchemaNameCollision { .. }
            | Error::OperationNameCollision { .. }
            | Error::InvalidTypeNameSuffix { .. }
            | Error::InvalidPathParameter { .. }
            | Error::UndeclaredPathParameter { .. }
            | Error::UnsupportedOperation { .. } => return None,
        }
    }
}

/// Convenience alias for results in this crate.
pub type Result<T> = std::result::Result<T, Error>;
