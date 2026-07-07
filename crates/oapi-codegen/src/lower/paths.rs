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
//! - **Cross-file `$ref` parameters, request bodies, and responses** — the
//!   referenced structural object is read from the sibling file (resolved
//!   relative to the main spec's directory), following chains across files.
//!   Parameter inner schemas must still resolve to scalars; body/response inner
//!   schema `$ref`s route through the `import-mapping` to an external type
//!   (they are never inlined).
//!
//! Rejected: object or other non-scalar parameters, non-default query-array
//! encodings, `content` parameters, array or `byte`/`binary` header parameters,
//! `byte`/`binary` cookie parameters, path-item `$ref`s, cross-file schema-type
//! `$ref`s (in a body or response) that lack an `import-mapping` entry for the
//! referenced file, and an unrecognised response status code.

use std::collections::BTreeMap;

use http::StatusCode as HttpStatus;
use openapiv3::Operation as OasOperation;
use openapiv3::Parameter;
use openapiv3::ParameterData;
use openapiv3::ParameterSchemaOrContent;
use openapiv3::QueryStyle;
use openapiv3::ReferenceOr;
use openapiv3::RequestBody;
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
use crate::loader::Resolved;
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

/// Rust field names the response emitter injects into a header-bearing struct
/// variant (`status` for dynamic responses, `body` when a body is present). A
/// declared response header whose `snake_case` identifier equals one of these
/// would collide, so such headers are rejected during lowering.
const RESERVED_RESPONSE_FIELDS: [&str; 2] = ["status", "body"];

/// Check whether a header name is valid for use with `HeaderName::from_static`.
/// Enforces the HTTP `tchar` token set (RFC 9110 §5.6.2 / RFC 7230): ASCII
/// alphanumerics plus ``!#$%&'*+-.^_`|~``. This prevents a later panic when
/// emitting `HeaderName::from_static`.
fn is_valid_header_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    for byte in name.as_bytes() {
        // RFC 7230 tchar. `-`..`9` (0x2D..0x39) would wrongly include `/`
        // (0x2F), which is not a valid header-name char, so digits are their
        // own range and `-`/`.` are listed explicitly.
        let valid = matches!(
            byte,
            b'!' | b'#'..=b'\'' | b'*'..=b'+' | b'-' | b'.' | b'0'..=b'9' | b'A'..=b'Z' | b'^'..=b'z' | b'|' | b'~'
        );
        if !valid {
            return false;
        }
    }
    return true;
}

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

        let params = self.resolve_parameters(operation, shared_params)?;
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
    /// parameters) into concrete `Parameter`s paired with the referenced file
    /// each was resolved from (`None` for inline or same-document entries), so
    /// inner schema `$ref`s can later be interpreted against the right document.
    /// Operation-level entries precede shared ones, preserving override order.
    fn resolve_parameters(
        &self,
        operation: &OasOperation,
        shared_params: &[ReferenceOr<Parameter>],
    ) -> Result<Vec<Resolved<Parameter>>> {
        let mut resolved = Vec::new();
        for parameter in operation.parameters.iter().chain(shared_params) {
            let entry = match parameter {
                ReferenceOr::Item(param) => Resolved {
                    value: param.clone(),
                    origin: None,
                },
                ReferenceOr::Reference { reference } => self.spec.resolve_parameter(reference)?,
            };
            resolved.push(entry);
        }
        return Ok(resolved);
    }

    /// Resolve the typed path parameters in their path-template order, which is
    /// the order axum extracts a `Path<(..)>` tuple in.
    fn lower_path_params(&self, path: &str, method: &str, params: &[Resolved<Parameter>]) -> Result<Vec<Param>> {
        let mut path_params = Vec::new();
        for name in path_param_names(path) {
            let declared = path_param_schema(&name, params);
            let ty = match declared {
                Some((format, origin)) => self.param_type(path, method, &name, origin, format)?,
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
        params: &[Resolved<Parameter>],
        operation_name: &RustIdent,
    ) -> Result<Option<Struct>> {
        let mut fields = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for parameter in params {
            let Parameter::Query {
                parameter_data, style, ..
            } = &parameter.value
            else {
                continue;
            };
            if seen.contains(&parameter_data.name.as_str()) {
                continue;
            }
            seen.push(&parameter_data.name);
            fields.push(self.query_field(path, method, parameter.origin.as_deref(), parameter_data, style)?);
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
    fn query_field(
        &self,
        path: &str,
        method: &str,
        origin: Option<&str>,
        data: &ParameterData,
        style: &QueryStyle,
    ) -> Result<Field> {
        let mut ty = self.query_param_type(path, method, origin, data, style)?;
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
        origin: Option<&str>,
        data: &ParameterData,
        style: &QueryStyle,
    ) -> Result<RustType> {
        let name = data.name.as_str();
        let explode = data.explode;
        let schema = match &data.format {
            ParameterSchemaOrContent::Schema(schema) => schema,
            ParameterSchemaOrContent::Content(_) => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("query parameter `{name}` uses `content`, which is not supported"),
                });
            }
        };
        let schema = self.resolve_param_schema(path, method, origin, name, schema)?;
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
            let element = match &array.items {
                Some(ReferenceOr::Item(item)) => scalar_type(&item.schema_kind),
                Some(ReferenceOr::Reference { reference }) if ref_file_part(reference).is_some() => {
                    return Err(Error::UnsupportedOperation {
                        method: method.to_owned(),
                        path: path.to_owned(),
                        reason: format!(
                            "query parameter `{name}` uses array items via a cross-file `$ref`, which is not supported"
                        ),
                    });
                }
                Some(ReferenceOr::Reference { reference }) => {
                    let item = self.spec.resolve_schema(origin, reference)?;
                    scalar_type(&item.schema_kind)
                }
                None => {
                    return Err(Error::UnsupportedOperation {
                        method: method.to_owned(),
                        path: path.to_owned(),
                        reason: format!("query parameter `{name}` is an array without `items`"),
                    });
                }
            };
            let element = element.ok_or_else(|| {
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

    /// Resolve a parameter schema reference to an owned concrete schema. A
    /// same-document reference is resolved against the main document, or against
    /// the referenced document the parameter came from (`origin`); a cross-file
    /// inner `$ref` is out of scope and rejected.
    fn resolve_param_schema(
        &self,
        path: &str,
        method: &str,
        origin: Option<&str>,
        name: &str,
        schema: &ReferenceOr<Schema>,
    ) -> Result<Schema> {
        match schema {
            ReferenceOr::Item(schema) => return Ok(schema.clone()),
            ReferenceOr::Reference { reference } if ref_file_part(reference).is_some() => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("query parameter `{name}` uses a cross-file `$ref`, which is not supported"),
                });
            }
            ReferenceOr::Reference { reference } => return self.spec.resolve_schema(origin, reference),
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
        params: &[Resolved<Parameter>],
        operation_name: &RustIdent,
    ) -> Result<Option<Headers>> {
        let mut header_params = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for parameter in params {
            let Parameter::Header { parameter_data, .. } = &parameter.value else {
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
            header_params.push(self.header_param(path, method, parameter.origin.as_deref(), parameter_data)?);
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
    fn header_param(
        &self,
        path: &str,
        method: &str,
        origin: Option<&str>,
        data: &ParameterData,
    ) -> Result<HeaderParam> {
        let ty = self.header_param_type(path, method, origin, &data.name, &data.format)?;
        return Ok(HeaderParam {
            name: to_ident(&data.name, Case::Snake),
            header_name: data.name.clone(),
            ty,
            required: data.required,
            doc: data.description.as_deref().and_then(trimmed),
        });
    }

    /// Map a header/response-header schema to a scalar Rust type, applying the
    /// shared rules: reject `content`, non-scalar shapes, and `byte`/`binary`;
    /// resolve a same-document/origin schema `$ref` to a scalar; reject a
    /// cross-file schema `$ref`. `kind_label` is used in error messages (e.g.
    /// "header parameter" or "response header").
    fn scalar_from_format(
        &self,
        path: &str,
        method: &str,
        origin: Option<&str>,
        kind_label: &str,
        name: &str,
        format: &ParameterSchemaOrContent,
    ) -> Result<RustType> {
        let schema = match format {
            ParameterSchemaOrContent::Schema(schema) => schema,
            ParameterSchemaOrContent::Content(_) => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("{kind_label} `{name}` uses `content`, which is not supported"),
                });
            }
        };
        let schema = match schema {
            ReferenceOr::Item(schema) => schema.clone(),
            ReferenceOr::Reference { reference } if ref_file_part(reference).is_some() => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("{kind_label} `{name}` uses a cross-file `$ref`, which is not supported"),
                });
            }
            ReferenceOr::Reference { reference } => self.spec.resolve_schema(origin, reference)?,
        };
        let ty = scalar_type(&schema.schema_kind).ok_or_else(|| {
            return Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("{kind_label} `{name}` must be a scalar"),
            };
        })?;
        if matches!(ty, RustType::Bytes) {
            return Err(Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("{kind_label} `{name}` uses a `byte`/`binary` format, which is not supported"),
            });
        }
        return Ok(ty);
    }

    /// Map a header parameter's schema to a scalar Rust type. `content`,
    /// cross-file `$ref`s, non-scalar shapes (arrays/objects), and `byte`/
    /// `binary` strings (which have no `FromStr`) are rejected.
    fn header_param_type(
        &self,
        path: &str,
        method: &str,
        origin: Option<&str>,
        name: &str,
        format: &ParameterSchemaOrContent,
    ) -> Result<RustType> {
        return self.scalar_from_format(path, method, origin, "header parameter", name, format);
    }

    /// Lower an operation's cookie parameters into a generated [`Cookies`]
    /// struct, returning `None` when the operation declares none. Per OpenAPI's
    /// override rule the first (operation-level) definition wins on a name
    /// collision. Cookies have no reserved-name analogue, so none are skipped.
    fn lower_cookie_params(
        &self,
        path: &str,
        method: &str,
        params: &[Resolved<Parameter>],
        operation_name: &RustIdent,
    ) -> Result<Option<Cookies>> {
        let mut cookie_params = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for parameter in params {
            let Parameter::Cookie { parameter_data, .. } = &parameter.value else {
                continue;
            };
            let name = parameter_data.name.as_str();
            if seen.contains(&name) {
                continue;
            }
            seen.push(name);
            let ty = self.cookie_param_type(
                path,
                method,
                parameter.origin.as_deref(),
                &parameter_data.name,
                &parameter_data.format,
            )?;
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
        origin: Option<&str>,
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
            ReferenceOr::Item(schema) => schema.clone(),
            ReferenceOr::Reference { reference } if ref_file_part(reference).is_some() => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("cookie parameter `{name}` uses a cross-file `$ref`, which is not supported"),
                });
            }
            ReferenceOr::Reference { reference } => self.spec.resolve_schema(origin, reference)?,
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

/// Locate the `path` parameter named `name` within a resolved parameter list,
/// returning its schema/content and the referenced file it was resolved from.
fn path_param_schema<'a>(
    name: &str,
    params: &'a [Resolved<Parameter>],
) -> Option<(&'a ParameterSchemaOrContent, Option<&'a str>)> {
    for parameter in params {
        let Parameter::Path { parameter_data, .. } = &parameter.value else {
            continue;
        };
        if parameter_data.name == name {
            return Some((&parameter_data.format, parameter.origin.as_deref()));
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
    /// Map a path parameter's schema to a scalar Rust type. Path parameters must
    /// be scalars (they are parsed from URL segments into an axum `Path<..>`
    /// tuple), so a `$ref` is resolved to its concrete schema and the scalar-only
    /// rule is enforced — the same as header and cookie parameters. A
    /// same-document `$ref` is resolved against the main document, or against the
    /// referenced document the parameter came from (`origin`); a cross-file inner
    /// `$ref` is rejected.
    fn param_type(
        &self,
        path: &str,
        method: &str,
        name: &str,
        origin: Option<&str>,
        format: &ParameterSchemaOrContent,
    ) -> Result<RustType> {
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
            ReferenceOr::Item(schema) => schema.clone(),
            ReferenceOr::Reference { reference } if ref_file_part(reference).is_some() => {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("path parameter `{name}` uses a cross-file `$ref`, which is not supported"),
                });
            }
            ReferenceOr::Reference { reference } => self.spec.resolve_schema(origin, reference)?,
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

    /// Lower an operation's JSON request body, if it declares one. A cross-file
    /// wrapper `$ref` is resolved against the referenced file; its inner schema
    /// `$ref`s are then interpreted against that file (`origin`).
    fn lower_request_body(&self, path: &str, method: &str, operation: &OasOperation) -> Result<Option<RustType>> {
        let body = match &operation.request_body {
            Some(body) => body,
            None => return Ok(None),
        };
        let (body, origin): (RequestBody, Option<String>) = match body {
            ReferenceOr::Item(body) => (body.clone(), None),
            ReferenceOr::Reference { reference } => {
                let resolved = self.spec.resolve_request_body(reference)?;
                (resolved.value, resolved.origin)
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
        let ty = self.body_type(path, method, origin.as_deref(), schema)?;
        return Ok(Some(ty));
    }

    /// Lower a response's declared headers into scalar-typed [`ResponseHeader`]s.
    /// Inline `Header` objects only; a `Header` that is itself a `$ref` is
    /// rejected. De-duplicated by case-insensitive name, first-seen winning.
    fn lower_response_headers(
        &self,
        path: &str,
        method: &str,
        origin: Option<&str>,
        response: &OasResponse,
    ) -> Result<Vec<crate::ir::ResponseHeader>> {
        let mut headers = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut seen_idents: Vec<String> = Vec::new();
        for (header_name, header_ref) in &response.headers {
            let header = match header_ref {
                ReferenceOr::Item(header) => header,
                ReferenceOr::Reference { .. } => {
                    return Err(Error::UnsupportedOperation {
                        method: method.to_owned(),
                        path: path.to_owned(),
                        reason: format!("response header `{header_name}` uses a `$ref`, which is not supported"),
                    });
                }
            };
            if seen.iter().any(|other| return other.eq_ignore_ascii_case(header_name)) {
                continue;
            }
            if !is_valid_header_name(header_name) {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!("response header `{header_name}` has an invalid header name"),
                });
            }
            let ident = to_ident(header_name, Case::Snake);
            // The response emitter injects `status` (dynamic responses) and
            // `body` (responses with a body) fields into the struct variant. A
            // header whose Rust field name collides with one of those would emit
            // duplicate fields. Reject rather than mis-generate.
            if RESERVED_RESPONSE_FIELDS.contains(&ident.logical()) {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!(
                        "response header `{header_name}` maps to the reserved Rust field name `{}`",
                        ident.logical()
                    ),
                });
            }
            // Distinct header names can collapse to the same Rust field
            // identifier (e.g. `X-Foo` and `X_Foo` both → `x_foo`), which would
            // emit a struct with duplicate fields. Reject rather than
            // mis-generate.
            if seen_idents.iter().any(|other| return other == ident.logical()) {
                return Err(Error::UnsupportedOperation {
                    method: method.to_owned(),
                    path: path.to_owned(),
                    reason: format!(
                        "response header `{header_name}` maps to the same Rust field name as another header (`{}`)",
                        ident.logical()
                    ),
                });
            }
            seen.push(header_name.clone());
            seen_idents.push(ident.logical().to_owned());
            let ty = self.scalar_from_format(path, method, origin, "response header", header_name, &header.format)?;
            headers.push(crate::ir::ResponseHeader {
                name: ident,
                header_name: header_name.clone(),
                ty,
                required: header.required,
                doc: header.description.as_deref().and_then(trimmed),
            });
        }
        return Ok(headers);
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
            let body = self.response_body(path, method, response.origin.as_deref(), &response.value)?;
            let headers = self.lower_response_headers(path, method, response.origin.as_deref(), &response.value)?;
            cases.push(ResponseCase {
                variant,
                status,
                body,
                headers,
                doc: trimmed(&response.value.description),
            });
        }

        if let Some(default) = &operation.responses.default {
            let response = self.resolve_response_ref(default)?;
            let body = self.response_body(path, method, response.origin.as_deref(), &response.value)?;
            let headers = self.lower_response_headers(path, method, response.origin.as_deref(), &response.value)?;
            cases.push(ResponseCase {
                variant: to_ident("default", Case::Pascal),
                status: ResponseStatus::Default,
                body,
                headers,
                doc: trimmed(&response.value.description),
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

    /// Resolve a possibly-referenced response to an owned [`OasResponse`] plus
    /// the referenced file it came from (`None` for inline/same-document).
    fn resolve_response_ref(&self, response: &ReferenceOr<OasResponse>) -> Result<Resolved<OasResponse>> {
        match response {
            ReferenceOr::Item(response) => {
                return Ok(Resolved {
                    value: response.clone(),
                    origin: None,
                });
            }
            ReferenceOr::Reference { reference } => return self.spec.resolve_response(reference),
        }
    }

    /// Extract a response's JSON body type, if it declares `application/json`
    /// content. Inner schema `$ref`s are interpreted against the response's
    /// origin file when it was resolved from a referenced document.
    fn response_body(
        &self,
        path: &str,
        method: &str,
        origin: Option<&str>,
        response: &OasResponse,
    ) -> Result<Option<RustType>> {
        let schema = response.content.get(JSON_MEDIA_TYPE).and_then(|media| {
            return media.schema.as_ref();
        });
        match schema {
            Some(schema) => return Ok(Some(self.body_type(path, method, origin, schema)?)),
            None => return Ok(None),
        }
    }

    /// Map a request/response body schema to a Rust type. Composite inline
    /// schemas must be referenced by name (`$ref`) so the models pass owns
    /// their emission. `origin` is the referenced file the enclosing wrapper was
    /// resolved from, so a same-document inner `$ref` lowers to the right module.
    fn body_type(
        &self,
        path: &str,
        method: &str,
        origin: Option<&str>,
        schema: &ReferenceOr<Schema>,
    ) -> Result<RustType> {
        match schema {
            ReferenceOr::Reference { reference } => return self.schema_ref_type(path, method, origin, reference),
            ReferenceOr::Item(schema) => return self.inline_body_type(path, method, origin, schema),
        }
    }

    /// Map an inline (non-`$ref`) body schema to a Rust type.
    fn inline_body_type(&self, path: &str, method: &str, origin: Option<&str>, schema: &Schema) -> Result<RustType> {
        let ty = match &schema.schema_kind {
            SchemaKind::Type(Type::String(st)) => string_format_type(&st.format),
            SchemaKind::Type(Type::Integer(it)) => integer_format_type(&it.format),
            SchemaKind::Type(Type::Number(_)) => RustType::F64,
            SchemaKind::Type(Type::Boolean(_)) => RustType::Bool,
            SchemaKind::Type(Type::Array(at)) => {
                let element = match &at.items {
                    Some(ReferenceOr::Reference { reference }) => {
                        self.schema_ref_type(path, method, origin, reference)?
                    }
                    Some(ReferenceOr::Item(item)) => self.inline_body_type(path, method, origin, item)?,
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

    /// Decide the Rust type for a schema `$ref`, given the referenced file the
    /// enclosing structural object was resolved from (`origin`). A cross-file
    /// ref, or a same-document ref whose enclosing object came from a referenced
    /// file, resolves through the `import-mapping` to a [`RustType::External`];
    /// a same-document ref in the main document stays a local [`RustType::Named`].
    fn schema_ref_type(&self, path: &str, method: &str, origin: Option<&str>, reference: &str) -> Result<RustType> {
        let target = ref_component_name(reference, "schemas").ok_or_else(|| {
            return Error::UnsupportedOperation {
                method: method.to_owned(),
                path: path.to_owned(),
                reason: format!("reference `{reference}` must point at a component schema"),
            };
        })?;
        let file = ref_file_part(reference)
            .map(str::to_owned)
            .or_else(|| return origin.map(str::to_owned));
        let Some(file) = file else {
            return Ok(RustType::Named(target.to_owned()));
        };
        let module = self.import_mapping.get(&file).ok_or_else(|| {
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
    fn valid_header_names_accept_tokens_and_reject_separators() {
        // Real header names with `-` and digits and `.` are accepted.
        assert!(is_valid_header_name("X-Request-Id"));
        assert!(is_valid_header_name("X-RateLimit-Remaining"));
        assert!(is_valid_header_name("Sec-CH-UA-Platform-Version"));
        assert!(is_valid_header_name("a.b"));
        // Empty and separator characters (which would panic `from_static`) are
        // rejected — notably `/` (0x2F), which sits between `-` (0x2D) and the
        // digits, and `:`, space, and control-ish punctuation.
        assert!(!is_valid_header_name(""));
        assert!(!is_valid_header_name("X/Y"));
        assert!(!is_valid_header_name("X:Y"));
        assert!(!is_valid_header_name("X Y"));
        assert!(!is_valid_header_name("X(Y)"));
    }

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
