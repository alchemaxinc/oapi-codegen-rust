//! Lowering OpenAPI paths/operations into the server [`crate::ir::Service`].
//!
//! Each operation is lowered into typed inputs (path/query/header parameters and
//! a JSON request body) plus a response enum. The generator only models what it
//! can translate faithfully; anything else is rejected with an error rather than
//! mis-generated.
//!
//! Supported:
//!
//! - **Path parameters** — inline scalars, or a same-document `$ref` to a scalar.
//! - **Query parameters** — scalars and arrays of scalars, lowered into a
//!   per-operation `Deserialize` struct extracted via
//!   `axum_extra::extract::Query`. Required parameters stay bare; optional ones
//!   become `Option<..>`. Arrays must use the default `form`/`explode: true`
//!   encoding (repeated keys).
//! - **Header parameters** — scalars only, lowered into a per-operation struct
//!   extracted via a generated `FromRequestParts` impl. The reserved
//!   `Accept`/`Content-Type`/`Authorization` headers are ignored.
//! - **Cookie parameters** — scalars only, lowered into a per-operation struct
//!   extracted via a generated `FromRequestParts` impl backed by
//!   `axum_extra`'s `CookieJar`.
//! - **Component `$ref` parameters and request bodies** — `$ref`s to
//!   `#/components/parameters/*` and `#/components/requestBodies/*` are resolved
//!   against the document.
//! - **Responses** — explicit status codes, the `default` catch-all, and ranges
//!   (`5XX`); component `$ref` responses are resolved against the document.
//! - **Cross-file `$ref`s** in bodies and responses are routed through the
//!   `import-mapping`.
//!
//! Rejected: object or other non-scalar parameters, non-default query-array
//! encodings, `content` parameters, array or `byte`/`binary` header parameters,
//! `byte`/`binary` cookie parameters, cross-file `$ref` parameters and
//! request-body wrappers, and an unrecognised response status code.

use std::collections::BTreeMap;

use http::StatusCode as HttpStatus;
use openapiv3::Operation as OasOperation;
use openapiv3::Parameter;
use openapiv3::ParameterData;
use openapiv3::ParameterSchemaOrContent;
use openapiv3::QueryStyle;
use openapiv3::ReferenceOr;
use openapiv3::Response as OasResponse;
use openapiv3::Schema;
use openapiv3::SchemaKind;
use openapiv3::StatusCode;
use openapiv3::Type;

use crate::error::Error;
use crate::error::Result;
use crate::ir::CookieParam;
use crate::ir::Cookies;
use crate::ir::Field;
use crate::ir::HeaderParam;
use crate::ir::Headers;
use crate::ir::Operation;
use crate::ir::Param;
use crate::ir::ResponseCase;
use crate::ir::ResponseStatus;
use crate::ir::RustType;
use crate::ir::Service;
use crate::ir::Struct;
use crate::loader::Spec;
use crate::loader::ref_component_name;
use crate::loader::ref_file_part;
use crate::lower::schema::integer_format_type;
use crate::lower::schema::string_format_type;
use crate::naming::Case;
use crate::naming::RustIdent;
use crate::naming::operations;
use crate::naming::to_ident;

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
        let response_enum = operations::response_enum_name(&name);

        let params = self.resolve_parameters(path, method, operation, shared_params)?;
        let path_params = self.lower_path_params(path, method, &params)?;
        let query = self.lower_query_params(path, method, &params, &name)?;
        let headers = self.lower_header_params(path, method, &params, &name)?;
        let cookies = self.lower_cookie_params(path, method, &params, &name)?;
        let body = self.lower_request_body(path, method, operation)?;
        let responses = self.lower_responses(path, method, operation)?;

        return Ok(Operation {
            name,
            response_enum,
            doc: operation_doc(operation),
            method: method.to_owned(),
            path: path.to_owned(),
            path_params,
            query,
            headers,
            cookies,
            body,
            responses,
        });
    }

    /// Resolve an operation's parameters (its own, then the path-item's shared
    /// parameters) into concrete `Parameter`s. `Item` entries pass through;
    /// same-document component `$ref`s resolve via the loader; a cross-file
    /// parameter `$ref` is rejected (deferred to cross-file parameter support).
    /// Operation-level entries precede shared ones, preserving override order.
    fn resolve_parameters(
        &self,
        path: &str,
        method: &str,
        operation: &OasOperation,
        shared_params: &[ReferenceOr<Parameter>],
    ) -> Result<Vec<Parameter>> {
        let mut resolved = Vec::new();
        for parameter in operation.parameters.iter().chain(shared_params) {
            let concrete = match parameter {
                ReferenceOr::Item(param) => param.clone(),
                ReferenceOr::Reference { reference } if ref_file_part(reference).is_some() => {
                    return Err(Error::UnsupportedOperation {
                        method: method.to_owned(),
                        path: path.to_owned(),
                        reason: format!("parameter `$ref` `{reference}` is cross-file, which is not supported"),
                    });
                }
                ReferenceOr::Reference { reference } => self.spec.resolve_parameter(reference)?.clone(),
            };
            resolved.push(concrete);
        }
        return Ok(resolved);
    }

    /// Resolve the typed path parameters in their path-template order, which is
    /// the order axum extracts a `Path<(..)>` tuple in.
    fn lower_path_params(&self, path: &str, method: &str, params: &[Parameter]) -> Result<Vec<Param>> {
        let mut path_params = Vec::new();
        for name in path_param_names(path) {
            let declared = path_param_schema(&name, params);
            let ty = match declared {
                Some(format) => self.param_type(path, method, &name, format)?,
                None => RustType::String,
            };
            path_params.push(Param {
                name: to_ident(&name, Case::Snake),
                ty,
            });
        }
        return Ok(path_params);
    }

    /// Lower an operation's query parameters into a generated `Deserialize`
    /// struct, returning `None` when the operation declares none. Per OpenAPI's
    /// override rule, an operation-level parameter takes precedence over a
    /// path-item one with the same name, so duplicates are de-duplicated keeping
    /// the first (operation-level) definition.
    fn lower_query_params(
        &self,
        path: &str,
        method: &str,
        params: &[Parameter],
        operation_name: &RustIdent,
    ) -> Result<Option<Struct>> {
        let mut fields = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for parameter in params {
            let Parameter::Query {
                parameter_data, style, ..
            } = parameter
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
        let name = operations::query_struct_name(operation_name);
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
    /// struct, returning `None` when the operation declares none. Per OpenAPI's
    /// override rule the first (operation-level) definition wins on a
    /// case-insensitive name collision, and the `Accept`/`Content-Type`/
    /// `Authorization` headers the specification reserves are skipped.
    fn lower_header_params(
        &self,
        path: &str,
        method: &str,
        params: &[Parameter],
        operation_name: &RustIdent,
    ) -> Result<Option<Headers>> {
        let mut header_params = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for parameter in params {
            let Parameter::Header { parameter_data, .. } = parameter else {
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
            header_params.push(self.header_param(path, method, parameter_data)?);
        }
        if header_params.is_empty() {
            return Ok(None);
        }
        let name = operations::headers_struct_name(operation_name);
        return Ok(Some(Headers {
            name,
            params: header_params,
        }));
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

    /// Lower an operation's cookie parameters into a generated [`Cookies`]
    /// struct, returning `None` when the operation declares none. Per OpenAPI's
    /// override rule the first (operation-level) definition wins on a name
    /// collision. Cookies have no reserved-name analogue, so none are skipped.
    fn lower_cookie_params(
        &self,
        path: &str,
        method: &str,
        params: &[Parameter],
        operation_name: &RustIdent,
    ) -> Result<Option<Cookies>> {
        let mut cookie_params = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for parameter in params {
            let Parameter::Cookie { parameter_data, .. } = parameter else {
                continue;
            };
            let name = parameter_data.name.as_str();
            if seen.contains(&name) {
                continue;
            }
            seen.push(name);
            let ty = self.cookie_param_type(path, method, &parameter_data.name, &parameter_data.format)?;
            cookie_params.push(CookieParam {
                name: to_ident(&parameter_data.name, Case::Snake),
                cookie_name: parameter_data.name.clone(),
                ty,
                required: parameter_data.required,
                doc: parameter_data.description.as_deref().and_then(trimmed),
            });
        }
        if cookie_params.is_empty() {
            return Ok(None);
        }
        let name = operations::cookies_struct_name(operation_name);
        return Ok(Some(Cookies {
            name,
            params: cookie_params,
        }));
    }

    /// Map a cookie parameter's schema to a scalar Rust type. `content`,
    /// cross-file `$ref`s, non-scalar shapes (arrays/objects), and `byte`/
    /// `binary` strings (no `FromStr`) are rejected.
    fn cookie_param_type(
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
                    reason: format!("cookie parameter `{name}` uses `content`, which is not supported"),
                });
            }
        };
        let schema = match schema {
            ReferenceOr::Item(schema) => schema,
            ReferenceOr::Reference { reference } if ref_file_part(reference).is_some() => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("cookie parameter `{name}` uses a cross-file `$ref`, which is not supported"),
                });
            }
            ReferenceOr::Reference { reference } => self.spec.resolve(reference)?,
        };
        let ty = scalar_type(&schema.schema_kind).ok_or_else(|| {
            return Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("cookie parameter `{name}` must be a scalar"),
            };
        })?;
        if matches!(ty, RustType::Bytes) {
            return Err(Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("cookie parameter `{name}` uses a `byte`/`binary` format, which is not supported"),
            });
        }
        return Ok(ty);
    }
}

/// Locate the `path` parameter named `name` within a concrete parameter list.
fn path_param_schema<'a>(name: &str, params: &'a [Parameter]) -> Option<&'a ParameterSchemaOrContent> {
    for parameter in params {
        let Parameter::Path { parameter_data, .. } = parameter else {
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
            ReferenceOr::Reference { reference } if ref_file_part(reference).is_some() => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("request-body `$ref` `{reference}` is cross-file, which is not supported"),
                });
            }
            ReferenceOr::Reference { reference } => self.spec.resolve_request_body(reference)?,
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
    /// component `$ref` responses against the document. A fixed status code
    /// becomes a reason-named variant with a compile-time status constant; a
    /// range (`5XX` → `Status5xx`) or the `default` response becomes a variant
    /// that carries the `axum::http::StatusCode` the handler supplies at runtime.
    fn lower_responses(&self, path: &str, method: &str, operation: &OasOperation) -> Result<Vec<ResponseCase>> {
        let mut cases = Vec::new();
        for (status_code, response) in &operation.responses.responses {
            let (status, variant) = match status_code {
                StatusCode::Code(code) => {
                    let reason = HttpStatus::from_u16(*code).ok().and_then(|status| {
                        return status.canonical_reason();
                    });
                    let reason = reason.ok_or_else(|| {
                        return Error::UnsupportedOperation {
                            method: method.to_owned(),
                            path: path.to_owned(),
                            reason: format!("status code `{code}` is not a recognised HTTP status"),
                        };
                    })?;
                    (ResponseStatus::Fixed(*code), to_ident(reason, Case::Pascal))
                }
                StatusCode::Range(range) => {
                    if !(1..=5).contains(range) {
                        return Err(Error::UnsupportedOperation {
                            method: method.to_owned(),
                            path: path.to_owned(),
                            reason: format!("response range `{range}XX` is not a valid HTTP status class"),
                        });
                    }
                    let variant = to_ident(&format!("status_{range}xx"), Case::Pascal);
                    (ResponseStatus::Range(*range as u8), variant)
                }
            };
            let response = self.resolve_response_ref(response)?;
            let body = self.response_body(path, method, response)?;
            cases.push(ResponseCase {
                variant,
                status,
                body,
                doc: trimmed(&response.description),
            });
        }

        if let Some(default) = &operation.responses.default {
            let response = self.resolve_response_ref(default)?;
            let body = self.response_body(path, method, response)?;
            cases.push(ResponseCase {
                variant: to_ident("default", Case::Pascal),
                status: ResponseStatus::Default,
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

    /// Resolve a possibly-referenced response to a concrete [`OasResponse`].
    fn resolve_response_ref<'r>(&'r self, response: &'r ReferenceOr<OasResponse>) -> Result<&'r OasResponse> {
        match response {
            ReferenceOr::Item(response) => return Ok(response),
            ReferenceOr::Reference { reference } => return self.spec.resolve_response(reference),
        }
    }

    /// Extract a response's JSON body type, if it declares `application/json`
    /// content.
    fn response_body(&self, path: &str, method: &str, response: &OasResponse) -> Result<Option<RustType>> {
        let schema = response.content.get(JSON_MEDIA_TYPE).and_then(|media| {
            return media.schema.as_ref();
        });
        match schema {
            Some(schema) => return Ok(Some(self.body_type(path, method, schema)?)),
            None => return Ok(None),
        }
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
        return operations::operation_method_name(id);
    }
    let synthesised = format!("{method} {path}");
    return operations::operation_method_name(&synthesised);
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

    #[test]
    fn range_response_variants_are_derived_from_the_range_digit() {
        let variant = |range: u16| {
            return to_ident(&format!("status_{range}xx"), Case::Pascal)
                .logical()
                .to_owned();
        };
        assert_eq!(variant(4), "Status4xx");
        assert_eq!(variant(5), "Status5xx");
    }
}
