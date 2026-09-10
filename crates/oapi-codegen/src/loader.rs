//! Loading OpenAPI documents and resolving `$ref`s.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use indexmap::IndexMap;
use openapiv3::OpenAPI;
use openapiv3::Parameter;
use openapiv3::ReferenceOr;
use openapiv3::RequestBody;
use openapiv3::Response;
use openapiv3::Schema;

use crate::error::Error;
use crate::error::Result;
use crate::lower::direction::REQUEST_SUFFIX;
use crate::lower::direction::RESPONSE_SUFFIX;

/// Maximum `$ref` chain length before bailing out (cycle guard).
const MAX_REF_DEPTH: usize = 32;

/// The OpenAPI minor versions the generator reads. Every parsed document must
/// declare a patch release of one of them.
///
/// This is a list so that adding a version is one entry here. The gate and its
/// message both read it, so neither states a version of its own.
const SUPPORTED_SPEC_VERSIONS: [&str; 1] = ["3.0"];

/// Top-level keys that carry operations the generator cannot emit. A document
/// that declares one is rejected, because ignoring it emits no handler for any
/// operation inside it and reads as a document that declares none.
///
/// `webhooks` is a 3.1 key, so today the version gate rejects such a document
/// first and this list only reaches a 3.0 document that declares the key anyway.
/// Such a document is still ambiguous, and the generator does not guess. The check
/// stands on its own once a version that defines the key is supported.
const UNSUPPORTED_TOP_LEVEL_KEYS: [&str; 1] = ["webhooks"];

/// Shared empty schema map returned when a document has no components.
static EMPTY_SCHEMAS: std::sync::OnceLock<IndexMap<String, ReferenceOr<Schema>>> = std::sync::OnceLock::new();

/// Shared empty security-scheme map returned when a document has no components.
static EMPTY_SECURITY_SCHEMES: std::sync::OnceLock<IndexMap<String, ReferenceOr<openapiv3::SecurityScheme>>> =
    std::sync::OnceLock::new();

/// A resolved structural object plus the referenced file it came from.
#[derive(Debug, Clone)]
pub struct Resolved<T> {
    /// The concrete, owned object.
    pub value: T,
    /// The referenced file the object was ultimately resolved from, written
    /// exactly as it appears in the `$ref` (the `import-mapping` key), or `None`
    /// for a same-document / inline object.
    pub origin: Option<String>,
}

/// A loaded OpenAPI document plus its source path (for diagnostics).
#[derive(Debug)]
pub struct Spec {
    inner: OpenAPI,
    source: PathBuf,
    docs: RefCell<HashMap<PathBuf, Rc<OpenAPI>>>,
}

impl Spec {
    /// Load and parse an OpenAPI document from a YAML or JSON file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| {
            return Error::ReadSpec {
                path: path.display().to_string(),
                source,
            };
        })?;
        let document = path.display().to_string();
        // Parse to a `Value` first, then into `OpenAPI` from that one tree. The
        // typed form drops every key it does not know, so a key such as
        // `webhooks:` is only visible here. This costs no second parse of the
        // text.
        let value: serde_yaml::Value = serde_yaml::from_str(&text).map_err(|source| {
            return Error::ParseSpec {
                path: document.clone(),
                source,
            };
        })?;
        // Both checks read the untyped tree, and both run before the typed parse.
        // The version gate must, because a 3.1-only construct fails that parse
        // with a message that names a YAML shape and not a version.
        check_spec_version(&document, &value)?;
        check_top_level_keys(&value)?;
        crate::coverage::check(&document, &value)?;
        let inner: OpenAPI = serde_yaml::from_value(value).map_err(|source| {
            return Error::ParseSpec {
                path: document.clone(),
                source,
            };
        })?;
        return Ok(Spec {
            inner,
            source: path.to_path_buf(),
            docs: RefCell::new(HashMap::new()),
        });
    }

    /// Construct a spec directly from an already-parsed document (test helper).
    pub fn from_parts(inner: OpenAPI, source: PathBuf) -> Self {
        return Spec {
            inner,
            source,
            docs: RefCell::new(HashMap::new()),
        };
    }

    /// Parse (once, cached) the file referenced by a cross-file `$ref`,
    /// resolving `file` relative to the directory containing the main spec.
    fn document_for(&self, file: &str) -> Result<Rc<OpenAPI>> {
        let base = self.source.parent().unwrap_or_else(|| {
            return Path::new(".");
        });
        let path = base.join(file);
        if let Some(doc) = self.docs.borrow().get(&path) {
            return Ok(Rc::clone(doc));
        }
        let text = std::fs::read_to_string(&path).map_err(|source| {
            return Error::ReadRefFile {
                file: file.to_owned(),
                source,
            };
        })?;
        let value: serde_yaml::Value = serde_yaml::from_str(&text).map_err(|source| {
            return Error::ParseRefFile {
                file: file.to_owned(),
                source,
            };
        })?;
        // A referenced file is a document of its own and declares its own
        // version. A 3.1 fragment pulled into a 3.0 document is the same
        // ambiguity as a 3.1 root, so the same gate applies, and it applies
        // before the typed parse for the same reason.
        check_spec_version(file, &value)?;
        check_top_level_keys(&value)?;
        crate::coverage::check(file, &value)?;
        let parsed: OpenAPI = serde_yaml::from_value(value).map_err(|source| {
            return Error::ParseRefFile {
                file: file.to_owned(),
                source,
            };
        })?;
        let doc = Rc::new(parsed);
        self.docs.borrow_mut().insert(path, Rc::clone(&doc));
        return Ok(doc);
    }

    /// The source path the spec was loaded from.
    pub fn source(&self) -> &Path {
        return &self.source;
    }

    /// Apply the configured operation and schema filters, mutating the spec in
    /// place before lowering (see [`crate::filter`]).
    pub fn apply_filters(&mut self, opts: &crate::config::OutputOptions) {
        crate::filter::apply(&mut self.inner, opts);
    }

    /// The component schemas declared in the document, in document order.
    pub fn schemas(&self) -> &IndexMap<String, ReferenceOr<Schema>> {
        let empty = EMPTY_SCHEMAS.get_or_init(IndexMap::new);
        let schemas = self
            .inner
            .components
            .as_ref()
            .map(|c| {
                return &c.schemas;
            })
            .unwrap_or(empty);
        return schemas;
    }

    /// The paths (operations) declared in the document, in document order.
    pub fn paths(&self) -> &openapiv3::Paths {
        return &self.inner.paths;
    }

    /// The top-level `servers` declared in the document, in document order.
    pub fn servers(&self) -> &[openapiv3::Server] {
        return &self.inner.servers;
    }

    /// The document's global `security` requirements, if declared. An operation
    /// with no `security` of its own inherits these.
    pub fn global_security(&self) -> Option<&[openapiv3::SecurityRequirement]> {
        return self.inner.security.as_deref();
    }

    /// The security schemes declared under `components.securitySchemes`, in
    /// document order.
    pub fn security_schemes(&self) -> &IndexMap<String, ReferenceOr<openapiv3::SecurityScheme>> {
        let empty = EMPTY_SECURITY_SCHEMES.get_or_init(IndexMap::new);
        let schemes = self
            .inner
            .components
            .as_ref()
            .map(|c| {
                return &c.security_schemes;
            })
            .unwrap_or(empty);
        return schemes;
    }

    /// Resolve a `#/components/responses/<name>` (possibly cross-file)
    /// reference to an owned component response plus the file it came from,
    /// following reference chains within and across documents.
    pub fn resolve_response(&self, reference: &str) -> Result<Resolved<Response>> {
        let mut current = reference.to_owned();
        let mut origin: Option<String> = None;
        for _ in 0..MAX_REF_DEPTH {
            if let Some(file) = ref_file_part(&current) {
                origin = Some(file.to_owned());
            }
            let name = ref_component_name(&current, "responses").ok_or_else(|| {
                return Error::UnsupportedRef {
                    reference: current.clone(),
                    reason: "only `#/components/responses/<name>` references are supported".to_owned(),
                };
            })?;
            let entry = self.component_response(origin.as_deref(), &current, name)?;
            match entry {
                ReferenceOr::Item(response) => {
                    return Ok(Resolved {
                        value: response,
                        origin,
                    });
                }
                ReferenceOr::Reference { reference } => {
                    current = reference;
                }
            }
        }
        return Err(Error::UnresolvedRef(reference.to_owned()));
    }

    /// Resolve a `#/components/parameters/<name>` (possibly cross-file)
    /// reference to an owned component parameter plus the file it came from,
    /// following reference chains within and across documents.
    pub fn resolve_parameter(&self, reference: &str) -> Result<Resolved<Parameter>> {
        let mut current = reference.to_owned();
        let mut origin: Option<String> = None;
        for _ in 0..MAX_REF_DEPTH {
            if let Some(file) = ref_file_part(&current) {
                origin = Some(file.to_owned());
            }
            let name = ref_component_name(&current, "parameters").ok_or_else(|| {
                return Error::UnsupportedRef {
                    reference: current.clone(),
                    reason: "only `#/components/parameters/<name>` references are supported".to_owned(),
                };
            })?;
            let entry = self.component_parameter(origin.as_deref(), &current, name)?;
            match entry {
                ReferenceOr::Item(parameter) => {
                    return Ok(Resolved {
                        value: parameter,
                        origin,
                    });
                }
                ReferenceOr::Reference { reference } => {
                    current = reference;
                }
            }
        }
        return Err(Error::UnresolvedRef(reference.to_owned()));
    }

    /// Resolve a `#/components/requestBodies/<name>` (possibly cross-file)
    /// reference to an owned component request body plus the file it came from,
    /// following reference chains within and across documents.
    pub fn resolve_request_body(&self, reference: &str) -> Result<Resolved<RequestBody>> {
        let mut current = reference.to_owned();
        let mut origin: Option<String> = None;
        for _ in 0..MAX_REF_DEPTH {
            if let Some(file) = ref_file_part(&current) {
                origin = Some(file.to_owned());
            }
            let name = ref_component_name(&current, "requestBodies").ok_or_else(|| {
                return Error::UnsupportedRef {
                    reference: current.clone(),
                    reason: "only `#/components/requestBodies/<name>` references are supported".to_owned(),
                };
            })?;
            let entry = self.component_request_body(origin.as_deref(), &current, name)?;
            match entry {
                ReferenceOr::Item(body) => {
                    return Ok(Resolved { value: body, origin });
                }
                ReferenceOr::Reference { reference } => {
                    current = reference;
                }
            }
        }
        return Err(Error::UnresolvedRef(reference.to_owned()));
    }

    /// Resolve a `$ref` string to the concrete schema it names, following
    /// chains of references within this document.
    pub fn resolve(&self, reference: &str) -> Result<&Schema> {
        let mut current = reference.to_owned();
        for _ in 0..MAX_REF_DEPTH {
            let name = ref_target_name(&current).ok_or_else(|| {
                return Error::UnsupportedRef {
                    reference: current.clone(),
                    reason: "only `#/components/schemas/<name>` references are supported".to_owned(),
                };
            })?;
            let entry = self
                .schemas()
                .get(name)
                .ok_or_else(|| return Error::UnresolvedRef(current.clone()))?;
            match entry {
                ReferenceOr::Item(schema) => {
                    return Ok(schema);
                }
                ReferenceOr::Reference { reference } => {
                    current = reference.clone();
                }
            }
        }
        return Err(Error::UnresolvedRef(reference.to_owned()));
    }

    /// Resolve a same-document schema `$ref` to an owned schema, against the
    /// main document when `origin` is `None` or a referenced document otherwise.
    ///
    /// A cross-file (`file#/...`) inner schema ref is intentionally rejected: a
    /// schema *type* reference is emitted as a named external type through the
    /// `import-mapping` (see the lowering pass's `schema_ref_type`), never read
    /// and inlined. This method only resolves refs that stay within one
    /// document's own `#/components/schemas`.
    pub fn resolve_schema(&self, origin: Option<&str>, reference: &str) -> Result<Schema> {
        let mut current = reference.to_owned();
        for _ in 0..MAX_REF_DEPTH {
            if ref_file_part(&current).is_some() {
                return Err(Error::UnsupportedRef {
                    reference: current.clone(),
                    reason: "cross-file schema `$ref`s are not supported here".to_owned(),
                });
            }
            let name = ref_component_name(&current, "schemas").ok_or_else(|| {
                return Error::UnsupportedRef {
                    reference: current.clone(),
                    reason: "only `#/components/schemas/<name>` references are supported".to_owned(),
                };
            })?;
            let entry = self.component_schema(origin, &current, name)?;
            match entry {
                ReferenceOr::Item(schema) => {
                    return Ok(schema);
                }
                ReferenceOr::Reference { reference } => {
                    current = reference;
                }
            }
        }
        return Err(Error::UnresolvedRef(reference.to_owned()));
    }

    /// Look up a named component in the main document (`origin` is `None`) or a
    /// referenced document, returning an owned copy. `select` extracts the
    /// specific component map's entry from a document. `reference` is the full
    /// `$ref` fragment currently being resolved, reported verbatim in the
    /// unresolved-reference error so a miss points at the exact ref (kind,
    /// component, and file). The origin dispatch and error are shared across
    /// component kinds.
    fn component_lookup<T>(
        &self,
        origin: Option<&str>,
        reference: &str,
        select: impl Fn(&OpenAPI) -> Option<ReferenceOr<T>>,
    ) -> Result<ReferenceOr<T>> {
        let entry = match origin {
            None => select(&self.inner),
            Some(file) => {
                let doc = self.document_for(file)?;
                select(doc.as_ref())
            }
        };
        return entry.ok_or_else(|| return Error::UnresolvedRef(reference.to_owned()));
    }

    /// Look up a component response (see [`Self::component_lookup`]).
    fn component_response(&self, origin: Option<&str>, reference: &str, name: &str) -> Result<ReferenceOr<Response>> {
        return self.component_lookup(origin, reference, |doc| {
            return doc.components.as_ref().and_then(|components| {
                return components.responses.get(name).cloned();
            });
        });
    }

    /// Look up a component parameter (see [`Self::component_lookup`]).
    fn component_parameter(&self, origin: Option<&str>, reference: &str, name: &str) -> Result<ReferenceOr<Parameter>> {
        return self.component_lookup(origin, reference, |doc| {
            return doc.components.as_ref().and_then(|components| {
                return components.parameters.get(name).cloned();
            });
        });
    }

    /// Look up a component request body (see [`Self::component_lookup`]).
    fn component_request_body(
        &self,
        origin: Option<&str>,
        reference: &str,
        name: &str,
    ) -> Result<ReferenceOr<RequestBody>> {
        return self.component_lookup(origin, reference, |doc| {
            return doc.components.as_ref().and_then(|components| {
                return components.request_bodies.get(name).cloned();
            });
        });
    }

    /// Look up a component schema (see [`Self::component_lookup`]).
    fn component_schema(&self, origin: Option<&str>, reference: &str, name: &str) -> Result<ReferenceOr<Schema>> {
        return self.component_lookup(origin, reference, |doc| {
            return doc.components.as_ref().and_then(|components| {
                return components.schemas.get(name).cloned();
            });
        });
    }

    /// The name a schema in a referenced file takes in the crate the
    /// `import-mapping` points at.
    ///
    /// The two runs read the same document, so an `x-rust-name` there reaches
    /// both. Without this the run that generates the models emits the name the
    /// key gives, the run that generates the operations emits the schema name,
    /// and the composed crate does not build.
    ///
    /// A miss is an error. The name would otherwise reach the output and name a
    /// type the other crate never declares.
    ///
    /// Two names in the referenced file that give one Rust name are an error
    /// too. The run that writes the models separates them with a
    /// `type-name-suffix` it takes from its own config. This run cannot see that
    /// config, so it emits the plain name and reaches whichever of the two
    /// schemas kept it. That builds, and it carries the wrong type.
    ///
    /// A schema the direction pass splits is an error for the same reason. The
    /// other run emits two names there, and this one cannot tell which of the
    /// two an `import-mapping` reference means.
    pub fn external_schema_name(&self, file: &str, name: &str, reference: &str) -> Result<String> {
        let entry = self.component_schema(Some(file), reference, name)?;
        let chosen = external_name_of(&entry, name)?;
        let doc = self.document_for(file)?;
        let schemas = doc.components.as_ref().map(|components| return &components.schemas);
        let ident = crate::naming::to_ident(&chosen, crate::naming::Case::Pascal);
        if direction_split_schemas(&doc).contains(name) {
            let shape = ident.logical();
            return Err(Error::UnsupportedRef {
                reference: reference.to_owned(),
                reason: format!(
                    "`{name}` in `{file}` marks a property `readOnly` or `writeOnly`, so the run that \
                     generates that file emits it as `{shape}{REQUEST_SUFFIX}` and `{shape}{RESPONSE_SUFFIX}`. \
                     This run cannot tell which of the two an `import-mapping` reference means. Declare \
                     the schema in this document instead, or drop the mark"
                ),
            });
        }
        for (other, other_entry) in schemas.into_iter().flatten() {
            if other == name {
                continue;
            }
            let taken = external_name_of(other_entry, other)?;
            if crate::naming::to_ident(&taken, crate::naming::Case::Pascal).logical() == ident.logical() {
                return Err(Error::UnsupportedRef {
                    reference: reference.to_owned(),
                    reason: format!(
                        "`{file}` gives `{name}` and `{other}` the one Rust name `{}`",
                        ident.logical()
                    ),
                });
            }
        }
        return Ok(chosen);
    }
}

/// The schemas in `doc` that the direction pass splits into a request shape and
/// a response shape.
///
/// A schema qualifies when a property inside it carries a direction mark, or
/// when it reaches such a schema through a same-document `$ref`.
///
/// A mark on a schema itself does not qualify that schema. The direction pass
/// splits a model only when one of its properties is directional, so a marked
/// primitive keeps its one name and only its referrers split.
///
/// This reads the document alone, because both runs that compose a crate must
/// reach the same verdict. Only one of the two holds the operations.
fn direction_split_schemas(doc: &OpenAPI) -> std::collections::BTreeSet<String> {
    let mut marked = std::collections::BTreeSet::new();
    let Some(components) = doc.components.as_ref() else {
        return marked;
    };

    let mut directional = std::collections::BTreeSet::new();
    for (name, entry) in &components.schemas {
        if let ReferenceOr::Item(schema) = entry
            && (schema.schema_data.read_only || schema.schema_data.write_only)
        {
            directional.insert(name.clone());
        }
    }

    let mut referrers: HashMap<String, Vec<String>> = HashMap::new();
    for (name, entry) in &components.schemas {
        match entry {
            ReferenceOr::Item(schema) => {
                if schema_marks_a_direction(schema, &directional) {
                    marked.insert(name.clone());
                }
                let mut targets = Vec::new();
                schema_local_refs(schema, &mut targets);
                for target in targets {
                    referrers.entry(target).or_default().push(name.clone());
                }
            }
            // A component declared as a `$ref` is an alias. An alias to a split
            // model splits as well, so it has to reach the closure below.
            ReferenceOr::Reference { reference } => {
                if let Some(target) = ref_target_name(reference) {
                    referrers.entry(target.to_owned()).or_default().push(name.clone());
                }
            }
        }
    }

    let mut stack: Vec<String> = marked.iter().cloned().collect();
    while let Some(name) = stack.pop() {
        let Some(parents) = referrers.get(&name) else {
            continue;
        };
        for parent in parents {
            if marked.insert(parent.clone()) {
                stack.push(parent.clone());
            }
        }
    }
    return marked;
}

/// Whether a property of `schema`, or of a schema written inside it, carries a
/// direction mark.
///
/// `directional` holds the component schemas that set `readOnly` or `writeOnly`
/// on themselves. A property that names one of those carries the mark too.
fn schema_marks_a_direction(schema: &Schema, directional: &std::collections::BTreeSet<String>) -> bool {
    let mut found = declares_a_directional_property(schema, directional);
    walk_inline_schemas(schema, &mut |inner| {
        found = found || declares_a_directional_property(inner, directional);
    });
    return found;
}

/// Whether one schema declares a property that only one direction carries.
fn declares_a_directional_property(schema: &Schema, directional: &std::collections::BTreeSet<String>) -> bool {
    return object_properties(schema).iter().any(|entry| {
        return match entry {
            ReferenceOr::Item(inner) => inner.schema_data.read_only || inner.schema_data.write_only,
            ReferenceOr::Reference { reference } => {
                return ref_file_part(reference).is_none()
                    && ref_component_name(reference, "schemas").is_some_and(|name| return directional.contains(name));
            }
        };
    });
}

/// The schemas one schema declares as properties.
fn object_properties(schema: &Schema) -> Vec<ReferenceOr<Schema>> {
    let unbox = |entry: &ReferenceOr<Box<Schema>>| {
        return match entry {
            ReferenceOr::Item(inner) => ReferenceOr::Item((**inner).clone()),
            ReferenceOr::Reference { reference } => ReferenceOr::Reference {
                reference: reference.clone(),
            },
        };
    };
    return match &schema.schema_kind {
        openapiv3::SchemaKind::Type(openapiv3::Type::Object(object)) => {
            return object.properties.values().map(&unbox).collect();
        }
        openapiv3::SchemaKind::Any(any) => return any.properties.values().map(&unbox).collect(),
        openapiv3::SchemaKind::Type(_)
        | openapiv3::SchemaKind::OneOf { .. }
        | openapiv3::SchemaKind::AllOf { .. }
        | openapiv3::SchemaKind::AnyOf { .. }
        | openapiv3::SchemaKind::Not { .. } => Vec::new(),
    };
}

/// The same-document component schemas a schema names, at any depth.
fn schema_local_refs(schema: &Schema, out: &mut Vec<String>) {
    let mut collect = |entry: &ReferenceOr<Schema>| {
        if let ReferenceOr::Reference { reference } = entry
            && ref_file_part(reference).is_none()
            && let Some(name) = ref_component_name(reference, "schemas")
        {
            out.push(name.to_owned());
        }
    };
    for entry in inline_members(schema) {
        collect(&entry);
    }
    walk_inline_schemas(schema, &mut |inner| {
        for entry in inline_members(inner) {
            collect(&entry);
        }
    });
}

/// Apply `visit` to every schema written inline within `schema`, at any depth.
fn walk_inline_schemas(schema: &Schema, visit: &mut impl FnMut(&Schema)) {
    for entry in inline_members(schema) {
        if let ReferenceOr::Item(inner) = entry {
            visit(&inner);
            walk_inline_schemas(&inner, visit);
        }
    }
}

/// The schemas one schema holds directly: its properties, its element type, its
/// `additionalProperties`, and its composition members.
fn inline_members(schema: &Schema) -> Vec<ReferenceOr<Schema>> {
    let unbox = |entry: &ReferenceOr<Box<Schema>>| {
        return match entry {
            ReferenceOr::Item(inner) => ReferenceOr::Item((**inner).clone()),
            ReferenceOr::Reference { reference } => ReferenceOr::Reference {
                reference: reference.clone(),
            },
        };
    };
    let additional = |entry: &Option<openapiv3::AdditionalProperties>| {
        return match entry {
            Some(openapiv3::AdditionalProperties::Schema(inner)) => vec![(**inner).clone()],
            Some(openapiv3::AdditionalProperties::Any(_)) | None => Vec::new(),
        };
    };
    let mut members = Vec::new();
    match &schema.schema_kind {
        openapiv3::SchemaKind::Type(openapiv3::Type::Object(object)) => {
            members.extend(object.properties.values().map(&unbox));
            members.extend(additional(&object.additional_properties));
        }
        openapiv3::SchemaKind::Type(openapiv3::Type::Array(array)) => {
            members.extend(array.items.as_ref().map(&unbox));
        }
        openapiv3::SchemaKind::Type(_) => {}
        openapiv3::SchemaKind::OneOf { one_of } => members.extend(one_of.iter().cloned()),
        openapiv3::SchemaKind::AllOf { all_of } => members.extend(all_of.iter().cloned()),
        openapiv3::SchemaKind::AnyOf { any_of } => members.extend(any_of.iter().cloned()),
        openapiv3::SchemaKind::Not { not } => members.push((**not).clone()),
        openapiv3::SchemaKind::Any(any) => {
            members.extend(any.properties.values().map(&unbox));
            members.extend(additional(&any.additional_properties));
            members.extend(any.items.as_ref().map(&unbox));
            members.extend(any.one_of.iter().cloned());
            members.extend(any.all_of.iter().cloned());
            members.extend(any.any_of.iter().cloned());
            members.extend(any.not.as_ref().map(|not| return (**not).clone()));
        }
    }
    return members;
}

/// The name a schema in a referenced document declares for itself, honouring
/// `x-rust-name`.
fn external_name_of(entry: &ReferenceOr<Schema>, name: &str) -> Result<String> {
    let renamed = match entry {
        ReferenceOr::Item(schema) => {
            crate::lower::extension::str_value(&schema.schema_data.extensions, crate::naming::X_RUST_NAME, name)?
        }
        ReferenceOr::Reference { .. } => None,
    };
    return Ok(renamed.unwrap_or(name).to_owned());
}

/// Reject a document whose `openapi:` value is not a patch release of a minor
/// version in [`SUPPORTED_SPEC_VERSIONS`].
///
/// The parser behind the generator ignores the `openapi:` value, so without this
/// check it reads whatever subset of an unsupported document happens to match a
/// supported dialect, and reports nothing. A document that generates in part and
/// fails in part is worse than one that fails at once, because the part that
/// generates looks correct.
///
/// The check runs on the untyped tree, before the typed parse. A construct that
/// only a newer version defines fails that parse with a message about a YAML
/// shape and not about a version.
///
/// A document that declares no `openapi:` key, or declares it as a non-string,
/// passes here. The typed parse reports that with a message that points at a line.
fn check_spec_version(document: &str, value: &serde_yaml::Value) -> Result<()> {
    let Some(version) = value.get("openapi").and_then(serde_yaml::Value::as_str) else {
        return Ok(());
    };
    let version = version.trim();
    // A patch part is optional in practice, so `3.0` and `3.0.3` both pass. The
    // dot guards against a future `3.00` reading as `3.0`.
    let supported = SUPPORTED_SPEC_VERSIONS.iter().any(|minor| {
        return version == *minor || version.starts_with(&format!("{minor}."));
    });
    if supported {
        return Ok(());
    }
    let reads = SUPPORTED_SPEC_VERSIONS
        .iter()
        .map(|minor| {
            return format!("{minor}.x");
        })
        .collect::<Vec<_>>()
        .join(", ");
    return Err(Error::UnsupportedSpecVersion {
        document: document.to_owned(),
        version: version.to_owned(),
        hint: format!("The generator reads OpenAPI {reads} only."),
    });
}

/// Reject a document that declares a top-level key holding operations the
/// generator cannot emit.
///
/// This reads the untyped tree, because the typed `OpenAPI` form drops every key
/// it does not know and a dropped key cannot be reported. A document that is not
/// a mapping passes here. The typed parse that follows reports that shape with
/// its own message, which points at the line.
fn check_top_level_keys(value: &serde_yaml::Value) -> Result<()> {
    let Some(mapping) = value.as_mapping() else {
        return Ok(());
    };
    for key in UNSUPPORTED_TOP_LEVEL_KEYS {
        if mapping.contains_key(serde_yaml::Value::String(key.to_owned())) {
            return Err(Error::UnsupportedSpecKey {
                key: key.to_owned(),
                reason: "declares operations the generator cannot emit".to_owned(),
                hint: format!(
                    "Remove `{key}:`, or move its operations under `paths:`. Silently ignoring it emits no handler for any operation it holds."
                ),
            });
        }
    }
    return Ok(());
}

/// Extract the trailing schema name from a *same-document* `$ref`
/// (`#/components/schemas/Foo` -> `Foo`).
///
/// Cross-file references (for example `schemas/x.yaml#/components/schemas/Foo`) yield
/// `None`: the models pipeline and the same-document `$ref` resolver only handle
/// in-document schemas, so accepting a cross-file name here will risk emitting a
/// local `Named` type for what is actually external. The server generator reads
/// cross-file names via [`ref_component_name`] paired with [`ref_file_part`].
pub fn ref_target_name(reference: &str) -> Option<&str> {
    if ref_file_part(reference).is_some() {
        return None;
    }
    return ref_component_name(reference, "schemas");
}

/// The reason a schema `$ref` at `site` gives no name.
///
/// [`ref_target_name`] answers `None` for two different faults, and the remedy
/// differs. A cross-file ref names a schema and still fails, so a message that
/// asks the author to reference a schema misleads. Name the fault instead.
///
/// `site` reads into the sentence, for example `a property`.
pub fn schema_ref_reason(reference: &str, site: &str) -> String {
    if ref_file_part(reference).is_some() {
        return format!("a cross-file ref does not resolve at {site}");
    }
    return format!("{site} must reference `#/components/schemas/<name>`");
}

/// Extract the trailing component name of the given `kind` (`schemas`,
/// `responses`, `parameters`, or `requestBodies`) from a (possibly cross-file)
/// `$ref`.
pub fn ref_component_name<'a>(reference: &'a str, kind: &str) -> Option<&'a str> {
    let fragment = reference.split('#').nth(1).unwrap_or(reference);
    let prefix = match kind {
        "schemas" => "/components/schemas/",
        "responses" => "/components/responses/",
        "parameters" => "/components/parameters/",
        "requestBodies" => "/components/requestBodies/",
        _ => return None,
    };
    let name = fragment.strip_prefix(prefix)?;
    if name.is_empty() || name.contains('/') {
        return None;
    }
    return Some(name);
}

/// The file part of a cross-file `$ref` (the text before `#`), or `None` for a
/// same-document reference such as `#/components/schemas/Foo`.
pub fn ref_file_part(reference: &str) -> Option<&str> {
    return match reference.split_once('#') {
        Some((file, _fragment)) if !file.is_empty() => Some(file),
        _ => None,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(test_name: &str) -> Self {
            let unique = format!(
                "oapi-codegen-loader-{test_name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock should be after Unix epoch")
                    .as_nanos(),
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).expect("create test directory");
            return Self { path };
        }

        fn write(&self, file: &str, contents: &str) -> PathBuf {
            let path = self.path.join(file);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create parent directory");
            }
            std::fs::write(&path, contents).expect("write test file");
            return path;
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn parse_openapi(yaml: &str) -> OpenAPI {
        return serde_yaml::from_str(yaml).expect("parse OpenAPI document");
    }

    fn assert_query_parameter_name(parameter: &Parameter, expected: &str) {
        match parameter {
            Parameter::Query { parameter_data, .. } => {
                assert_eq!(parameter_data.name, expected);
            }
            _ => panic!("expected a query parameter"),
        }
    }

    fn minimal_doc() -> &'static str {
        return "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\n";
    }

    fn shared_parameter_doc(name: &str) -> String {
        return format!(
            "openapi: 3.0.3\ninfo:\n  title: shared\n  version: '1'\npaths: {{}}\ncomponents:\n  parameters:\n    PageSize:\n      name: {name}\n      in: query\n      schema:\n        type: integer\n",
        );
    }

    #[test]
    fn ref_target_name_is_same_document_schemas_only() {
        assert_eq!(ref_target_name("#/components/schemas/Foo"), Some("Foo"));
        // Cross-file schema refs are rejected here. The server path resolves
        // them via `ref_component_name` + `ref_file_part` instead.
        assert_eq!(
            ref_target_name("schemas/common.yaml#/components/schemas/ErrorResponse"),
            None
        );
        assert_eq!(ref_target_name("#/components/responses/Bar"), None);
    }

    /// The two faults behind a `None` from [`ref_target_name`] need different
    /// remedies, so the reason must tell them apart.
    #[test]
    fn a_schema_ref_reason_names_the_fault() {
        assert_eq!(
            schema_ref_reason("common.yaml#/components/schemas/X", "a property"),
            "a cross-file ref does not resolve at a property"
        );
        assert_eq!(
            schema_ref_reason("#/components/responses/Bar", "a property"),
            "a property must reference `#/components/schemas/<name>`"
        );
    }

    #[test]
    fn extracts_component_responses_and_file_parts() {
        assert_eq!(
            ref_component_name("#/components/responses/Bar", "responses"),
            Some("Bar")
        );
        assert_eq!(ref_component_name("#/components/schemas/Foo", "responses"), None);
        assert_eq!(ref_file_part("#/components/schemas/Foo"), None);
        assert_eq!(
            ref_file_part("schemas/common.yaml#/components/schemas/ErrorResponse"),
            Some("schemas/common.yaml"),
        );
    }

    #[test]
    fn ref_component_name_recognizes_parameters_and_request_bodies() {
        assert_eq!(
            ref_component_name("#/components/parameters/PageSize", "parameters"),
            Some("PageSize")
        );
        assert_eq!(
            ref_component_name("#/components/requestBodies/CreateWidget", "requestBodies"),
            Some("CreateWidget")
        );
        assert_eq!(ref_component_name("#/components/schemas/Foo", "parameters"), None);
    }

    #[test]
    fn resolves_same_document_component_parameter() {
        let yaml = "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\ncomponents:\n  parameters:\n    PageSize:\n      name: pageSize\n      in: query\n      schema:\n        type: integer\n";
        let doc = parse_openapi(yaml);
        let spec = Spec::from_parts(doc, std::path::PathBuf::from("inline.yaml"));
        let param = spec
            .resolve_parameter("#/components/parameters/PageSize")
            .expect("resolve");
        assert!(param.origin.is_none());
        assert_query_parameter_name(&param.value, "pageSize");
    }

    #[test]
    fn errors_on_cross_file_ref_to_missing_file() {
        let source = std::env::temp_dir().join(format!(
            "oapi-codegen-loader-missing-{}-{}.yaml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos(),
        ));
        let spec = Spec::from_parts(parse_openapi(minimal_doc()), source);
        let result = spec.resolve_parameter("common.yaml#/components/parameters/PageSize");
        assert!(matches!(result, Err(Error::ReadRefFile { .. })));
    }

    #[test]
    fn unresolved_component_error_preserves_the_full_reference() {
        // A same-document miss reports the full `$ref` fragment (kind + name),
        // not the bare component name.
        let yaml = "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\ncomponents:\n  parameters: {}\n";
        let spec = Spec::from_parts(parse_openapi(yaml), std::path::PathBuf::from("inline.yaml"));
        let result = spec.resolve_parameter("#/components/parameters/Missing");
        match result {
            Err(Error::UnresolvedRef(reference)) => {
                assert_eq!(reference, "#/components/parameters/Missing");
            }
            other => panic!("expected UnresolvedRef, got {other:?}"),
        }

        // A cross-file miss reports the file plus the full fragment.
        let dir = TestDir::new("unresolved-cross-file");
        let main = dir.write("main.yaml", minimal_doc());
        dir.write("shared.yaml", minimal_doc());
        let spec = Spec::load(&main).expect("load main spec");
        let result = spec.resolve_parameter("shared.yaml#/components/parameters/Missing");
        match result {
            Err(Error::UnresolvedRef(reference)) => {
                assert_eq!(reference, "shared.yaml#/components/parameters/Missing");
            }
            other => panic!("expected UnresolvedRef, got {other:?}"),
        }
    }

    #[test]
    fn document_for_reads_sibling_and_caches() {
        let dir = TestDir::new("document-for-caches");
        let main = dir.write("main.yaml", minimal_doc());
        dir.write("shared.yaml", &shared_parameter_doc("pageSize"));

        let spec = Spec::load(&main).expect("load main spec");
        let param = spec
            .resolve_parameter("shared.yaml#/components/parameters/PageSize")
            .expect("resolve cross-file parameter");
        assert_eq!(param.origin.as_deref(), Some("shared.yaml"));
        assert_query_parameter_name(&param.value, "pageSize");
        assert_eq!(spec.docs.borrow().len(), 1);

        let second = spec
            .resolve_parameter("shared.yaml#/components/parameters/PageSize")
            .expect("resolve cached cross-file parameter");
        assert_eq!(second.origin.as_deref(), Some("shared.yaml"));
        assert_query_parameter_name(&second.value, "pageSize");
        assert_eq!(spec.docs.borrow().len(), 1);
    }

    #[test]
    fn cross_file_chain_across_two_files_resolves() {
        let dir = TestDir::new("cross-file-chain");
        let main = dir.write("main.yaml", minimal_doc());
        dir.write(
            "a.yaml",
            "openapi: 3.0.3\ninfo:\n  title: a\n  version: '1'\npaths: {}\ncomponents:\n  parameters:\n    X:\n      $ref: \"b.yaml#/components/parameters/Y\"\n",
        );
        dir.write(
            "b.yaml",
            "openapi: 3.0.3\ninfo:\n  title: b\n  version: '1'\npaths: {}\ncomponents:\n  parameters:\n    Y:\n      name: cursor\n      in: query\n      schema:\n        type: string\n",
        );

        let spec = Spec::load(&main).expect("load main spec");
        let param = spec
            .resolve_parameter("a.yaml#/components/parameters/X")
            .expect("resolve cross-file chain");
        assert_eq!(param.origin.as_deref(), Some("b.yaml"));
        assert_query_parameter_name(&param.value, "cursor");
    }

    #[test]
    fn cross_file_cycle_terminates() {
        let dir = TestDir::new("cross-file-cycle");
        let main = dir.write("main.yaml", minimal_doc());
        dir.write(
            "a.yaml",
            "openapi: 3.0.3\ninfo:\n  title: a\n  version: '1'\npaths: {}\ncomponents:\n  parameters:\n    X:\n      $ref: \"b.yaml#/components/parameters/Y\"\n",
        );
        dir.write(
            "b.yaml",
            "openapi: 3.0.3\ninfo:\n  title: b\n  version: '1'\npaths: {}\ncomponents:\n  parameters:\n    Y:\n      $ref: \"a.yaml#/components/parameters/X\"\n",
        );

        let spec = Spec::load(&main).expect("load main spec");
        let result = spec.resolve_parameter("a.yaml#/components/parameters/X");
        assert!(matches!(result, Err(Error::UnresolvedRef(_))));
    }

    #[test]
    fn resolve_schema_against_origin() {
        let dir = TestDir::new("resolve-schema-origin");
        let main = dir.write("main.yaml", minimal_doc());
        dir.write(
            "shared.yaml",
            "openapi: 3.0.3\ninfo:\n  title: shared\n  version: '1'\npaths: {}\ncomponents:\n  schemas:\n    PageInfo:\n      type: integer\n      format: int32\n",
        );

        let spec = Spec::load(&main).expect("load main spec");
        let schema = spec
            .resolve_schema(Some("shared.yaml"), "#/components/schemas/PageInfo")
            .expect("resolve schema from origin");
        assert!(matches!(
            schema.schema_kind,
            openapiv3::SchemaKind::Type(openapiv3::Type::Integer(_))
        ));
    }

    /// A mark on a schema itself splits the schemas that name it, not the schema.
    ///
    /// The direction pass splits a model only when one of its properties is
    /// directional, so a marked primitive stays one type alias. Reporting it as
    /// split rejects an `import-mapping` reference that resolves.
    #[test]
    fn a_schema_level_mark_splits_only_the_referrers() {
        let doc: OpenAPI = serde_yaml::from_str(
            "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\ncomponents:\n  schemas:\n    Marked:\n      type: string\n      readOnly: true\n    Holder:\n      type: object\n      properties:\n        a:\n          $ref: '#/components/schemas/Marked'\n    Bystander:\n      type: object\n      properties:\n        b:\n          type: string\n",
        )
        .expect("parse doc");

        let split = direction_split_schemas(&doc);
        assert!(split.contains("Holder"), "a property that names a marked schema splits");
        assert!(!split.contains("Marked"), "a marked primitive keeps its one name");
        assert!(!split.contains("Bystander"), "an unrelated schema keeps its one name");
    }

    /// A component declared as a `$ref` is an alias, and an alias to a split
    /// model splits as well.
    ///
    /// Missing one emits a reference to a name the models run never writes, so
    /// the composed crate does not build.
    #[test]
    fn an_alias_to_a_split_model_splits_as_well() {
        let doc: OpenAPI = serde_yaml::from_str(
            "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\ncomponents:\n  schemas:\n    Marked:\n      type: object\n      properties:\n        id:\n          type: string\n          readOnly: true\n    Alias:\n      $ref: '#/components/schemas/Marked'\n    Plain:\n      type: object\n      properties:\n        a:\n          type: string\n    PlainAlias:\n      $ref: '#/components/schemas/Plain'\n",
        )
        .expect("parse doc");

        let split = direction_split_schemas(&doc);
        assert!(split.contains("Marked"), "the model that declares the mark splits");
        assert!(split.contains("Alias"), "an alias to a split model splits");
        assert!(!split.contains("Plain"), "an unmarked model keeps its one name");
        assert!(
            !split.contains("PlainAlias"),
            "an alias to an unmarked model keeps its one name"
        );
    }

    /// A mark on a property splits the model that declares it, and every model
    /// that reaches it.
    #[test]
    fn a_property_mark_splits_the_declaring_model_and_its_referrers() {
        let doc: OpenAPI = serde_yaml::from_str(
            "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\ncomponents:\n  schemas:\n    Leaf:\n      type: object\n      properties:\n        id:\n          type: string\n          readOnly: true\n    Parent:\n      type: object\n      properties:\n        leaf:\n          $ref: '#/components/schemas/Leaf'\n",
        )
        .expect("parse doc");

        let split = direction_split_schemas(&doc);
        assert!(split.contains("Leaf"), "the model that declares the mark splits");
        assert!(split.contains("Parent"), "a model that reaches the mark splits");
    }
}
