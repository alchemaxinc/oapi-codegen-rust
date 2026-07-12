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

/// Maximum `$ref` chain length before bailing out (cycle guard).
const MAX_REF_DEPTH: usize = 32;

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
        let inner: OpenAPI = serde_yaml::from_str(&text).map_err(|source| {
            return Error::ParseSpec {
                path: path.display().to_string(),
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
        let parsed: OpenAPI = serde_yaml::from_str(&text).map_err(|source| {
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
    /// specific component map's entry from a document; `reference` is the full
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
}

/// Extract the trailing schema name from a *same-document* `$ref`
/// (`#/components/schemas/Foo` -> `Foo`).
///
/// Cross-file references (e.g. `schemas/x.yaml#/components/schemas/Foo`) yield
/// `None`: the models pipeline and the same-document `$ref` resolver only handle
/// in-document schemas, so accepting a cross-file name here would risk emitting a
/// local `Named` type for what is actually external. The server generator reads
/// cross-file names via [`ref_component_name`] paired with [`ref_file_part`].
pub fn ref_target_name(reference: &str) -> Option<&str> {
    if ref_file_part(reference).is_some() {
        return None;
    }
    return ref_component_name(reference, "schemas");
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
        // Cross-file schema refs are rejected here; the server path resolves
        // them via `ref_component_name` + `ref_file_part` instead.
        assert_eq!(
            ref_target_name("schemas/common.yaml#/components/schemas/ErrorResponse"),
            None
        );
        assert_eq!(ref_target_name("#/components/responses/Bar"), None);
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
        // not just the bare component name.
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
}
