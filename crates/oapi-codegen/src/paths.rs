//! Lowering OpenAPI paths/operations into the server [`crate::ir::Service`].
//!
//! The current slice covers typed path parameters, query parameters (lowered
//! into a per-operation `Deserialize` struct extracted via
//! `axum_extra::extract::Query`), a JSON request body, and explicit-status
//! responses. Component `$ref` responses are resolved against the document and
//! cross-file schema `$ref`s are routed through the `import-mapping`. Query
//! parameters accept scalars and arrays of scalars (required ones stay bare,
//! optional ones become `Option<..>`); array parameters must use OpenAPI's
//! default `form`/`explode: true` encoding (repeated keys). Object/non-scalar
//! query shapes, non-default array encodings, `content` parameters, and
//! cross-file `$ref` query parameters are rejected. Header parameters are
//! lowered into a per-operation struct extracted via a generated
//! `FromRequestParts` impl (scalars only; arrays/objects, `content`, cross-file
//! `$ref`s, and `byte`/`binary` formats are rejected, and the reserved
//! `Accept`/`Content-Type`/`Authorization` headers are ignored). Cookie
//! parameters, `default`/range responses, and component-level `$ref`s for
//! parameters and request bodies are intentionally not handled yet and are
//! rejected explicitly.

use std::collections::BTreeMap;

use http::StatusCode as HttpStatus;
use openapiv3::Operation as OasOperation;
use openapiv3::Parameter;
use openapiv3::ParameterData;
use openapiv3::ParameterSchemaOrContent;
use openapiv3::QueryStyle;
use openapiv3::ReferenceOr;
use openapiv3::Schema;
use openapiv3::SchemaKind;
use openapiv3::StatusCode;
use openapiv3::Type;

use crate::error::Error;
use crate::error::Result;
use crate::ir::Field;
use crate::ir::HeaderParam;
use crate::ir::Headers;
use crate::ir::Operation;
use crate::ir::Param;
use crate::ir::ResponseCase;
use crate::ir::RustType;
use crate::ir::Service;
use crate::ir::Struct;
use crate::loader::Spec;
use crate::loader::ref_component_name;
use crate::loader::ref_file_part;
use crate::naming::Case;
use crate::naming::RustIdent;
use crate::naming::to_ident;
use crate::schema::integer_format_type;
use crate::schema::string_format_type;

/// The JSON media type the slice reads request and response bodies from.
const JSON_MEDIA_TYPE: &str = "application/json";

/// Header parameter names that OpenAPI mandates be ignored when declared with
/// `in: header`, since they are governed by content negotiation / security
/// mechanisms rather than the parameter object (compared case-insensitively).
const IGNORED_HEADER_NAMES: [&str; 3] = ["accept", "content-type", "authorization"];

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
        let query = self.lower_query_params(path, method, operation, shared_params, &name)?;
        let headers = self.lower_header_params(path, method, operation, shared_params, &name)?;
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
            query,
            headers,
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

    /// Lower an operation's query parameters into a generated `Deserialize`
    /// struct, returning `None` when the operation declares none. Component
    /// parameter `$ref`s are already rejected by [`Self::lower_path_params`], so
    /// only inline `Parameter::Query` entries are considered here. Per OpenAPI's
    /// override rule, an operation-level parameter takes precedence over a
    /// path-item one with the same name, so duplicates are de-duplicated keeping
    /// the first (operation-level) definition.
    fn lower_query_params(
        &self,
        path: &str,
        method: &str,
        operation: &OasOperation,
        shared_params: &[ReferenceOr<Parameter>],
        operation_name: &RustIdent,
    ) -> Result<Option<Struct>> {
        let mut fields = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for parameter in operation.parameters.iter().chain(shared_params) {
            let ReferenceOr::Item(Parameter::Query {
                parameter_data, style, ..
            }) = parameter
            else {
                continue;
            };
            if seen.contains(&parameter_data.name.as_str()) {
                continue;
            }
            seen.push(&parameter_data.name);
            fields.push(self.query_field(path, method, parameter_data, style)?);
        }
        if fields.is_empty() {
            return Ok(None);
        }
        let name = to_ident(&format!("{}_query", operation_name.logical()), Case::Pascal);
        return Ok(Some(Struct {
            name,
            doc: None,
            fields,
            additional_properties: None,
        }));
    }

    /// Build a query struct field from a single query parameter's metadata,
    /// wrapping optional parameters in `Option<..>`.
    fn query_field(&self, path: &str, method: &str, data: &ParameterData, style: &QueryStyle) -> Result<Field> {
        let mut ty = self.query_param_type(path, method, &data.name, &data.format, style, data.explode)?;
        if !data.required {
            ty = ty.optional();
        }
        let ident = to_ident(&data.name, Case::Snake);
        let rename = crate::naming::rename_for(&data.name, &ident);
        return Ok(Field {
            name: ident,
            rename,
            doc: data.description.as_deref().and_then(trimmed),
            ty,
            required: data.required,
        });
    }

    /// Map a query parameter's schema to a scalar Rust type, or a `Vec<T>` of
    /// scalars. Cross-file `$ref`s, `content`, and non-scalar shapes (including
    /// arrays of non-scalars) are rejected. Array parameters must use OpenAPI's
    /// default `form`/`explode: true` encoding (repeated keys), since the
    /// generated server reads them through `axum-extra`'s `Query` extractor;
    /// other array encodings are rejected rather than silently mis-parsed.
    fn query_param_type(
        &self,
        path: &str,
        method: &str,
        name: &str,
        format: &ParameterSchemaOrContent,
        style: &QueryStyle,
        explode: Option<bool>,
    ) -> Result<RustType> {
        let schema = match format {
            ParameterSchemaOrContent::Schema(schema) => schema,
            ParameterSchemaOrContent::Content(_) => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("query parameter `{name}` uses `content`, which is not supported"),
                });
            }
        };
        let schema = self.resolve_param_schema(path, method, name, schema)?;
        if let SchemaKind::Type(Type::Array(array)) = &schema.schema_kind {
            if !matches!(style, QueryStyle::Form) || explode == Some(false) {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!(
                        "query parameter `{name}` uses a non-default array encoding; only `style: form` with `explode: true` (repeated keys) is supported"
                    ),
                });
            }
            let item = match &array.items {
                Some(ReferenceOr::Item(item)) => item.as_ref(),
                Some(ReferenceOr::Reference { reference }) if ref_file_part(reference).is_some() => {
                    return Err(Error::UnsupportedOperation {
                        method: method.to_owned(),
                        path: path.to_owned(),
                        reason: format!(
                            "query parameter `{name}` uses array items via a cross-file `$ref`, which is not supported"
                        ),
                    });
                }
                Some(ReferenceOr::Reference { reference }) => self.spec.resolve(reference)?,
                None => {
                    return Err(Error::UnsupportedOperation {
                        method: method.to_owned(),
                        path: path.to_owned(),
                        reason: format!("query parameter `{name}` is an array without `items`"),
                    });
                }
            };
            let element = scalar_type(&item.schema_kind).ok_or_else(|| {
                return Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("query parameter `{name}` must be an array of scalars"),
                };
            })?;
            return Ok(RustType::Vec(Box::new(element)));
        }
        let ty = scalar_type(&schema.schema_kind).ok_or_else(|| {
            return Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("query parameter `{name}` must be a scalar or an array of scalars"),
            };
        })?;
        return Ok(ty);
    }

    /// Resolve a parameter schema reference to a concrete schema, rejecting
    /// cross-file `$ref`s: query parameters only route same-document references.
    fn resolve_param_schema<'s>(
        &'s self,
        path: &str,
        method: &str,
        name: &str,
        schema: &'s ReferenceOr<Schema>,
    ) -> Result<&'s Schema> {
        match schema {
            ReferenceOr::Item(schema) => return Ok(schema),
            ReferenceOr::Reference { reference } if ref_file_part(reference).is_some() => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("query parameter `{name}` uses a cross-file `$ref`, which is not supported"),
                });
            }
            ReferenceOr::Reference { reference } => return self.spec.resolve(reference),
        }
    }

    /// Lower an operation's header parameters into a generated [`Headers`]
    /// struct, returning `None` when the operation declares none. Only inline
    /// `Parameter::Header` entries are considered (component parameter `$ref`s
    /// are already rejected by [`Self::lower_path_params`]). Per OpenAPI's
    /// override rule the first (operation-level) definition wins on a
    /// case-insensitive name collision, and the `Accept`/`Content-Type`/
    /// `Authorization` headers the specification reserves are skipped.
    fn lower_header_params(
        &self,
        path: &str,
        method: &str,
        operation: &OasOperation,
        shared_params: &[ReferenceOr<Parameter>],
        operation_name: &RustIdent,
    ) -> Result<Option<Headers>> {
        let mut params = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for parameter in operation.parameters.iter().chain(shared_params) {
            let ReferenceOr::Item(Parameter::Header { parameter_data, .. }) = parameter else {
                continue;
            };
            let name = parameter_data.name.as_str();
            if IGNORED_HEADER_NAMES
                .iter()
                .any(|ignored| return ignored.eq_ignore_ascii_case(name))
            {
                continue;
            }
            if seen.iter().any(|other| return other.eq_ignore_ascii_case(name)) {
                continue;
            }
            seen.push(name);
            params.push(self.header_param(path, method, parameter_data)?);
        }
        if params.is_empty() {
            return Ok(None);
        }
        let name = to_ident(&format!("{}_headers", operation_name.logical()), Case::Pascal);
        return Ok(Some(Headers { name, params }));
    }

    /// Build a single header field, resolving its scalar type and recording the
    /// exact header name for the generated case-insensitive lookup.
    fn header_param(&self, path: &str, method: &str, data: &ParameterData) -> Result<HeaderParam> {
        let ty = self.header_param_type(path, method, &data.name, &data.format)?;
        return Ok(HeaderParam {
            name: to_ident(&data.name, Case::Snake),
            header_name: data.name.clone(),
            ty,
            required: data.required,
            doc: data.description.as_deref().and_then(trimmed),
        });
    }

    /// Map a header parameter's schema to a scalar Rust type. `content`,
    /// cross-file `$ref`s, non-scalar shapes (arrays/objects), and `byte`/
    /// `binary` strings (which have no `FromStr`) are rejected.
    fn header_param_type(
        &self,
        path: &str,
        method: &str,
        name: &str,
        format: &ParameterSchemaOrContent,
    ) -> Result<RustType> {
        let schema = match format {
            ParameterSchemaOrContent::Schema(schema) => schema,
            ParameterSchemaOrContent::Content(_) => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("header parameter `{name}` uses `content`, which is not supported"),
                });
            }
        };
        let schema = match schema {
            ReferenceOr::Item(schema) => schema,
            ReferenceOr::Reference { reference } if ref_file_part(reference).is_some() => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("header parameter `{name}` uses a cross-file `$ref`, which is not supported"),
                });
            }
            ReferenceOr::Reference { reference } => self.spec.resolve(reference)?,
        };
        let ty = scalar_type(&schema.schema_kind).ok_or_else(|| {
            return Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("header parameter `{name}` must be a scalar"),
            };
        })?;
        if matches!(ty, RustType::Bytes) {
            return Err(Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("header parameter `{name}` uses a `byte`/`binary` format, which is not supported"),
            });
        }
        return Ok(ty);
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

/// Map an OpenAPI schema kind to its Rust type when it is one of the four
/// supported scalars (`string`, `integer`, `number`, `boolean`), else `None`.
fn scalar_type(kind: &SchemaKind) -> Option<RustType> {
    let ty = match kind {
        SchemaKind::Type(Type::String(st)) => string_format_type(&st.format),
        SchemaKind::Type(Type::Integer(it)) => integer_format_type(&it.format),
        SchemaKind::Type(Type::Number(_)) => RustType::F64,
        SchemaKind::Type(Type::Boolean(_)) => RustType::Bool,
        _ => return None,
    };
    return Some(ty);
}

impl Lowerer<'_> {
    /// Map a path parameter's schema to a scalar Rust type.
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
            // Cross-file parameter refs route through import-mapping; same-document
            // refs are resolved here so the scalar-only rule below still applies.
            ReferenceOr::Reference { reference } if ref_file_part(reference).is_some() => {
                return self.named_from_ref(path, method, reference);
            }
            ReferenceOr::Reference { reference } => self.spec.resolve(reference)?,
            ReferenceOr::Item(schema) => schema,
        };
        let ty = scalar_type(&schema.schema_kind).ok_or_else(|| {
            return Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("path parameter `{name}` must be a scalar type"),
            };
        })?;
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
