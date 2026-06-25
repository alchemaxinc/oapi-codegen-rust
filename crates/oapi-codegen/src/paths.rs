//! Lowering OpenAPI paths/operations into the server [`crate::ir::Service`].
//!
//! The current slice covers typed path parameters, a JSON request body, and
//! explicit-status responses. Component `$ref` responses are resolved against
//! the document and cross-file schema `$ref`s are routed through the
//! `import-mapping`. Query/header/cookie parameters, `default`/range responses,
//! and component-level `$ref`s for parameters and request bodies are
//! intentionally not handled yet and are rejected explicitly.

use std::collections::BTreeMap;

use http::StatusCode as HttpStatus;
use openapiv3::Operation as OasOperation;
use openapiv3::Parameter;
use openapiv3::ParameterSchemaOrContent;
use openapiv3::ReferenceOr;
use openapiv3::Schema;
use openapiv3::SchemaKind;
use openapiv3::StatusCode;
use openapiv3::Type;

use crate::error::Error;
use crate::error::Result;
use crate::ir::Operation;
use crate::ir::Param;
use crate::ir::ResponseCase;
use crate::ir::RustType;
use crate::ir::Service;
use crate::loader::Spec;
use crate::loader::ref_component_name;
use crate::loader::ref_file_part;
use crate::naming::Case;
use crate::naming::to_ident;
use crate::schema::integer_format_type;
use crate::schema::string_format_type;

/// The JSON media type the slice reads request and response bodies from.
const JSON_MEDIA_TYPE: &str = "application/json";

/// Lower every operation in `spec` into the server IR, resolving cross-file
/// schema references through `import_mapping`.
pub fn generate_service(spec: &Spec, import_mapping: &BTreeMap<String, String>) -> Result<Service> {
    let lowerer = Lowerer { spec, import_mapping };
    return lowerer.lower();
}

/// Carries the document and its import mapping through operation lowering.
struct Lowerer<'a> {
    spec: &'a Spec,
    import_mapping: &'a BTreeMap<String, String>,
}

impl Lowerer<'_> {
    /// Lower every operation in the document into the server IR.
    fn lower(&self) -> Result<Service> {
        let mut operations = Vec::new();
        for (path, entry) in self.spec.paths().iter() {
            let item = match entry {
                ReferenceOr::Item(item) => item,
                ReferenceOr::Reference { .. } => {
                    return Err(Error::UnsupportedOperation {
                        method: "*".to_owned(),
                        path: path.clone(),
                        reason: "path-item `$ref`s are not supported".to_owned(),
                    });
                }
            };
            for (method, operation) in item.iter() {
                let lowered = self.lower_operation(path, method, operation, &item.parameters)?;
                operations.push(lowered);
            }
        }
        return Ok(Service { operations });
    }

    /// Lower a single operation, given its path, method and path-item parameters.
    fn lower_operation(
        &self,
        path: &str,
        method: &str,
        operation: &OasOperation,
        shared_params: &[ReferenceOr<Parameter>],
    ) -> Result<Operation> {
        let name = operation_name(path, method, operation);
        let handler = to_ident(&format!("{}_handler", name.logical()), Case::Snake);
        let response_enum = to_ident(&format!("{}_response", name.logical()), Case::Pascal);

        let path_params = self.lower_path_params(path, method, operation, shared_params)?;
        let body = self.lower_request_body(path, method, operation)?;
        let responses = self.lower_responses(path, method, operation)?;

        return Ok(Operation {
            name,
            handler,
            response_enum,
            doc: operation_doc(operation),
            method: method.to_owned(),
            path: path.to_owned(),
            path_params,
            body,
            responses,
        });
    }

    /// Resolve the typed path parameters in their path-template order, which is
    /// the order axum extracts a `Path<(..)>` tuple in.
    fn lower_path_params(
        &self,
        path: &str,
        method: &str,
        operation: &OasOperation,
        shared_params: &[ReferenceOr<Parameter>],
    ) -> Result<Vec<Param>> {
        for parameter in operation.parameters.iter().chain(shared_params) {
            if matches!(parameter, ReferenceOr::Reference { .. }) {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: "component parameter `$ref`s are not supported".to_owned(),
                });
            }
        }

        let mut params = Vec::new();
        for name in path_param_names(path) {
            let declared = find_path_param(&name, operation, shared_params);
            let ty = match declared {
                Some(format) => self.param_type(path, method, &name, format)?,
                None => RustType::String,
            };
            params.push(Param {
                name: to_ident(&name, Case::Snake),
                ty,
            });
        }
        return Ok(params);
    }
}

/// Find a declared path parameter's schema by name, preferring the operation's
/// own parameters over the shared path-item parameters.
fn find_path_param<'a>(
    name: &str,
    operation: &'a OasOperation,
    shared_params: &'a [ReferenceOr<Parameter>],
) -> Option<&'a ParameterSchemaOrContent> {
    let from_operation = path_param_schema(name, &operation.parameters);
    if from_operation.is_some() {
        return from_operation;
    }
    return path_param_schema(name, shared_params);
}

/// Locate the inline `path` parameter named `name` within a parameter list.
fn path_param_schema<'a>(name: &str, parameters: &'a [ReferenceOr<Parameter>]) -> Option<&'a ParameterSchemaOrContent> {
    for parameter in parameters {
        let ReferenceOr::Item(Parameter::Path { parameter_data, .. }) = parameter else {
            continue;
        };
        if parameter_data.name == name {
            return Some(&parameter_data.format);
        }
    }
    return None;
}

/// Map a path parameter's schema to a scalar Rust type.
impl Lowerer<'_> {
    fn param_type(&self, path: &str, method: &str, name: &str, format: &ParameterSchemaOrContent) -> Result<RustType> {
        let schema = match format {
            ParameterSchemaOrContent::Schema(schema) => schema,
            ParameterSchemaOrContent::Content(_) => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("path parameter `{name}` uses `content`, which is not supported"),
                });
            }
        };
        let schema = match schema {
            ReferenceOr::Reference { reference } => return self.named_from_ref(path, method, reference),
            ReferenceOr::Item(schema) => schema,
        };
        let ty = match &schema.schema_kind {
            SchemaKind::Type(Type::String(st)) => string_format_type(&st.format),
            SchemaKind::Type(Type::Integer(it)) => integer_format_type(&it.format),
            SchemaKind::Type(Type::Number(_)) => RustType::F64,
            SchemaKind::Type(Type::Boolean(_)) => RustType::Bool,
            _ => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("path parameter `{name}` must be a scalar type"),
                });
            }
        };
        return Ok(ty);
    }

    /// Lower an operation's JSON request body, if it declares one.
    fn lower_request_body(&self, path: &str, method: &str, operation: &OasOperation) -> Result<Option<RustType>> {
        let body = match &operation.request_body {
            Some(body) => body,
            None => return Ok(None),
        };
        let body = match body {
            ReferenceOr::Item(body) => body,
            ReferenceOr::Reference { .. } => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: "component request-body `$ref`s are not supported".to_owned(),
                });
            }
        };
        let media = match body.content.get(JSON_MEDIA_TYPE) {
            Some(media) => media,
            None => return Ok(None),
        };
        let schema = match &media.schema {
            Some(schema) => schema,
            None => return Ok(None),
        };
        let ty = self.body_type(path, method, schema)?;
        return Ok(Some(ty));
    }

    /// Lower an operation's responses into typed enum variants, resolving
    /// component `$ref` responses against the document.
    fn lower_responses(&self, path: &str, method: &str, operation: &OasOperation) -> Result<Vec<ResponseCase>> {
        if operation.responses.default.is_some() {
            return Err(Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: "`default` responses are not supported yet".to_owned(),
            });
        }

        let mut cases = Vec::new();
        for (status_code, response) in &operation.responses.responses {
            let code = match status_code {
                StatusCode::Code(code) => *code,
                StatusCode::Range(range) => {
                    return Err(Error::UnsupportedOperation {
                        method: method.to_owned(),
                        path: path.to_owned(),
                        reason: format!("range response `{range}XX` is not supported yet"),
                    });
                }
            };
            let reason = HttpStatus::from_u16(code).ok().and_then(|status| {
                return status.canonical_reason();
            });
            let reason = reason.ok_or_else(|| {
                return Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("status code `{code}` is not a recognised HTTP status"),
                };
            })?;
            let response = match response {
                ReferenceOr::Item(response) => response,
                ReferenceOr::Reference { reference } => self.spec.resolve_response(reference)?,
            };
            let body = match response.content.get(JSON_MEDIA_TYPE).and_then(|media| {
                return media.schema.as_ref();
            }) {
                Some(schema) => Some(self.body_type(path, method, schema)?),
                None => None,
            };
            cases.push(ResponseCase {
                variant: to_ident(reason, Case::Pascal),
                status: code,
                body,
                doc: trimmed(&response.description),
            });
        }

        if cases.is_empty() {
            return Err(Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: "operation declares no responses".to_owned(),
            });
        }
        return Ok(cases);
    }

    /// Map a request/response body schema to a Rust type. Composite inline
    /// schemas must be referenced by name (`$ref`) so the models pass owns
    /// their emission.
    fn body_type(&self, path: &str, method: &str, schema: &ReferenceOr<Schema>) -> Result<RustType> {
        match schema {
            ReferenceOr::Reference { reference } => return self.named_from_ref(path, method, reference),
            ReferenceOr::Item(schema) => return self.inline_body_type(path, method, schema),
        }
    }

    /// Map an inline (non-`$ref`) body schema to a Rust type.
    fn inline_body_type(&self, path: &str, method: &str, schema: &Schema) -> Result<RustType> {
        let ty = match &schema.schema_kind {
            SchemaKind::Type(Type::String(st)) => string_format_type(&st.format),
            SchemaKind::Type(Type::Integer(it)) => integer_format_type(&it.format),
            SchemaKind::Type(Type::Number(_)) => RustType::F64,
            SchemaKind::Type(Type::Boolean(_)) => RustType::Bool,
            SchemaKind::Type(Type::Array(at)) => {
                let element = match &at.items {
                    Some(ReferenceOr::Reference { reference }) => self.named_from_ref(path, method, reference)?,
                    Some(ReferenceOr::Item(item)) => self.inline_body_type(path, method, item)?,
                    None => RustType::Value,
                };
                RustType::Vec(Box::new(element))
            }
            SchemaKind::Any(_) => RustType::Value,
            _ => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: "composite request/response bodies must reference a named schema (`$ref`)".to_owned(),
                });
            }
        };
        return Ok(ty);
    }

    /// Resolve a `$ref` string to a named type. Same-document references become
    /// a local [`RustType::Named`]; cross-file references are routed through the
    /// `import-mapping` to a [`RustType::External`].
    fn named_from_ref(&self, path: &str, method: &str, reference: &str) -> Result<RustType> {
        let target = ref_component_name(reference, "schemas").ok_or_else(|| {
            return Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("reference `{reference}` must point at a component schema"),
            };
        })?;
        let Some(file) = ref_file_part(reference) else {
            return Ok(RustType::Named(target.to_owned()));
        };
        let module = self.import_mapping.get(file).ok_or_else(|| {
            return Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("cross-file reference `{reference}` needs an `import-mapping` entry for `{file}`"),
            };
        })?;
        return Ok(RustType::External {
            module: module.clone(),
            name: target.to_owned(),
        });
    }
}

/// Derive the trait method name: the `operationId` if present, else a name
/// synthesised from the method and path (e.g. `get /v1/widgets` -> `get_v1_widgets`).
fn operation_name(path: &str, method: &str, operation: &OasOperation) -> crate::naming::RustIdent {
    if let Some(id) = &operation.operation_id {
        return to_ident(id, Case::Snake);
    }
    let synthesised = format!("{method} {path}");
    return to_ident(&synthesised, Case::Snake);
}

/// The operation's doc comment, preferring `summary` over `description`.
fn operation_doc(operation: &OasOperation) -> Option<String> {
    if let Some(summary) = &operation.summary
        && let Some(text) = trimmed(summary)
    {
        return Some(text);
    }
    return operation.description.as_ref().and_then(|text| {
        return trimmed(text);
    });
}

/// Extract `{name}` path-parameter names in their order of appearance.
fn path_param_names(path: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find('}') else {
            break;
        };
        names.push(after_open[..close].to_owned());
        rest = &after_open[close + 1..];
    }
    return names;
}

/// Trim a string and return `None` when it is empty.
fn trimmed(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    return Some(trimmed.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_path_param_names_in_order() {
        assert_eq!(path_param_names("/v1/widgets"), Vec::<String>::new());
        assert_eq!(path_param_names("/pets/{id}"), vec!["id".to_owned()]);
        assert_eq!(
            path_param_names("/orgs/{org}/pets/{petId}"),
            vec!["org".to_owned(), "petId".to_owned()],
        );
    }

    #[test]
    fn response_variants_are_named_after_the_status_reason() {
        let variant = |code| {
            let reason = HttpStatus::from_u16(code)
                .expect("test status code is a valid HTTP status")
                .canonical_reason()
                .expect("status code has a canonical reason phrase");
            return to_ident(reason, Case::Pascal).logical().to_owned();
        };
        assert_eq!(variant(200), "Ok");
        assert_eq!(variant(204), "NoContent");
        assert_eq!(variant(404), "NotFound");
        assert_eq!(variant(500), "InternalServerError");
    }
}
