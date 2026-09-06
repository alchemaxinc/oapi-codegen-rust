//! Error types for the generator.

/// Errors that can occur while loading a spec or generating code.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An OpenAPI object contains an invalid key or structure.
    InvalidSpec {
        /// The source document.
        document: String,
        /// The JSON pointer to the invalid entry.
        path: String,
        /// Why the entry is invalid.
        reason: String,
    },
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

    /// The companion directory beside the output file holds a file the
    /// generator did not write.
    ///
    /// The generator owns that directory: it deletes the files an earlier run
    /// left there and this run no longer produces. A file without the generated
    /// marker is somebody's work, so the run stops rather than delete it.
    UnownedOutput {
        /// Path of the file the generator does not own.
        path: String,
    },

    /// A generated file lies outside the directory the run owns.
    ///
    /// Every file of a package belongs under the companion directory, which is
    /// the only place a run writes to and deletes from.
    OutsideOutput {
        /// The offending file path, relative to the root file's directory.
        path: String,
        /// The companion directory the file has to be under.
        directory: String,
    },

    /// The output path cannot carry a companion directory beside it.
    ///
    /// A run with operations writes a root file plus a directory named after
    /// its stem. An output path with no stem, or one whose stem equals the file
    /// name, would put the directory and the root file at the same path.
    UnsplittableOutput {
        /// The output path that cannot be split.
        path: String,
    },

    /// The document declares an OpenAPI version the generator does not read.
    ///
    /// Only `3.0.x` is supported. A newer document is rejected and not read as a
    /// 3.0 document, because the dialects overlap: one whose every construct
    /// happens to parse as 3.0 would generate quietly, and one newer construct in
    /// the same file would fail with a `serde` message that names no version.
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

    /// An `x-` extension carried a value of a kind the generator cannot read.
    ///
    /// The author wrote the key to change something. A silent fallback to the
    /// default would hide that nothing changed.
    InvalidExtensionValue {
        /// The extension key, for example `x-rust-name`.
        key: String,
        /// The place the key sits, for example a schema or a server URL.
        at: String,
        /// The kind of value the key needs.
        expected: String,
        /// The kind of value the document gave.
        found: String,
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

    /// The schema `default` does not fit the Rust type of the field. Either the
    /// two disagree, or the value has no literal form here. A dropped default
    /// leaves the document and the code in disagreement.
    UnsupportedDefault {
        /// The type that owns the property.
        owner: String,
        /// The property's name as the document writes it.
        property: String,
        /// The offending `default`, as JSON.
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

    /// A generated type took the name of a Rust prelude type that the emitted
    /// code writes unqualified, such as `Option` or `Vec`.
    ///
    /// The name does not duplicate an emitted item, so no other collision check
    /// sees it. It shadows the prelude inside the generated file instead, and
    /// every use of the shadowed type there stops compiling.
    PreludeShadowing {
        /// The Rust type name that shadows the prelude.
        name: String,
        /// What generated code can name the shadowed type for, for example
        /// `every optional field`. The check reads names, not uses, so the file
        /// at hand does not have to hold one.
        used_for: String,
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

    /// Two or more type aliases refer to each other in a cycle.
    ///
    /// `type A = B; type B = A;` is a cycle rustc rejects with `E0391`, and no
    /// amount of indirection fixes it: a `Box` around either side still expands
    /// forever. The recursion pass boxes a struct field or a union variant, and
    /// a cycle made only of aliases offers neither.
    RecursiveAlias {
        /// The alias names on the cycle, in the order the walk met them.
        cycle: Vec<String>,
        /// How to break the cycle. Rendered by the console as a hint, and not by
        /// `Display`, so the console does not print it twice.
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
            Error::InvalidSpec { document, path, reason } => {
                return write!(f, "{document}#{path}: {reason}");
            }
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
            Error::OutsideOutput { path, directory } => {
                return write!(f, "generated file `{path}` lies outside `{directory}`");
            }
            Error::UnsplittableOutput { path } => {
                return write!(
                    f,
                    "output path `{path}` needs a file extension: a run with operations writes a directory beside the file, named after its stem"
                );
            }
            Error::UnownedOutput { path } => {
                return write!(
                    f,
                    "`{path}` sits in the generated output directory but was not generated"
                );
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
            Error::InvalidExtensionValue {
                key,
                at,
                expected,
                found,
            } => {
                return write!(f, "`{key}` on `{at}` needs {expected}, but the document gives {found}");
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
            Error::UnsupportedDefault {
                owner,
                property,
                declared,
                ..
            } => {
                return write!(
                    f,
                    "the `default` of `{owner}.{property}` cannot be represented as a value of the property's Rust type: {declared}"
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
            Error::PreludeShadowing { name, used_for, .. } => {
                return write!(
                    f,
                    "generated type `{name}` shadows the Rust prelude type of that name, which generated code can name without a path for {used_for}"
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
            Error::RecursiveAlias { cycle, .. } => {
                return write!(f, "type aliases refer to each other in a cycle: {}", cycle.join(" -> "));
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
            Error::InvalidSpec { .. }
            | Error::Validation { .. }
            | Error::Unimplemented(_)
            | Error::UnownedOutput { .. }
            | Error::UnsplittableOutput { .. }
            | Error::OutsideOutput { .. }
            | Error::UnsupportedSpecVersion { .. }
            | Error::UnsupportedSpecKey { .. }
            | Error::UnsupportedContentType { .. }
            | Error::UnsupportedDefault { .. }
            | Error::UnresolvedRef(_)
            | Error::UnsupportedRef { .. }
            | Error::InvalidExtensionValue { .. }
            | Error::UnsupportedSchema { .. }
            | Error::SchemaDepthExceeded { .. }
            | Error::TypeNameCollision { .. }
            | Error::DuplicateTypeName { .. }
            | Error::PreludeShadowing { .. }
            | Error::OperationTypeCollision { .. }
            | Error::SchemaNameCollision { .. }
            | Error::RecursiveAlias { .. }
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
