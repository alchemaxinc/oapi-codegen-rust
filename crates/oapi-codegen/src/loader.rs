//! Loading OpenAPI documents and resolving `$ref`s.

use std::path::Path;
use std::path::PathBuf;

use indexmap::IndexMap;
use openapiv3::OpenAPI;
use openapiv3::ReferenceOr;
use openapiv3::Response;
use openapiv3::Schema;

use crate::error::Error;
use crate::error::Result;

/// Maximum `$ref` chain length before bailing out (cycle guard).
const MAX_REF_DEPTH: usize = 32;

/// Shared empty schema map returned when a document has no components.
static EMPTY_SCHEMAS: std::sync::OnceLock<IndexMap<String, ReferenceOr<Schema>>> = std::sync::OnceLock::new();

/// A loaded OpenAPI document plus its source path (for diagnostics).
#[derive(Debug)]
pub struct Spec {
    inner: OpenAPI,
    source: PathBuf,
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
        });
    }

    /// Construct a spec directly from an already-parsed document (test helper).
    pub fn from_parts(inner: OpenAPI, source: PathBuf) -> Self {
        return Spec { inner, source };
    }

    /// The source path the spec was loaded from.
    pub fn source(&self) -> &Path {
        return &self.source;
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

    /// Resolve a `#/components/responses/<name>` reference to the concrete
    /// component response it names, following same-document reference chains.
    pub fn resolve_response(&self, reference: &str) -> Result<&Response> {
        let mut current = reference.to_owned();
        for _ in 0..MAX_REF_DEPTH {
            let name = ref_component_name(&current, "responses").ok_or_else(|| {
                return Error::UnsupportedRef {
                    reference: current.clone(),
                    reason: "only `#/components/responses/<name>` references are supported".to_owned(),
                };
            })?;
            let entry = self
                .inner
                .components
                .as_ref()
                .and_then(|components| {
                    return components.responses.get(name);
                })
                .ok_or_else(|| return Error::UnresolvedRef(current.clone()))?;
            match entry {
                ReferenceOr::Item(response) => {
                    return Ok(response);
                }
                ReferenceOr::Reference { reference } => {
                    current = reference.clone();
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
}

/// Extract the trailing schema name from a (possibly cross-file) `$ref`.
///
/// `#/components/schemas/Foo` and `schemas/x.yaml#/components/schemas/Foo` both
/// yield `Foo`.
pub fn ref_target_name(reference: &str) -> Option<&str> {
    return ref_component_name(reference, "schemas");
}

/// Extract the trailing component name of the given `kind` (e.g. `schemas`,
/// `responses`) from a (possibly cross-file) `$ref`.
pub fn ref_component_name<'a>(reference: &'a str, kind: &str) -> Option<&'a str> {
    let fragment = reference.split('#').nth(1).unwrap_or(reference);
    let prefix = format!("/components/{kind}/");
    let name = fragment.strip_prefix(&prefix)?;
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

    #[test]
    fn extracts_local_and_cross_file_ref_names() {
        assert_eq!(ref_target_name("#/components/schemas/Foo"), Some("Foo"));
        assert_eq!(
            ref_target_name("schemas/common.yaml#/components/schemas/ErrorResponse"),
            Some("ErrorResponse"),
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
}
