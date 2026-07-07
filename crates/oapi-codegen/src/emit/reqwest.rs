//! Emitting the blocking `reqwest` client as token streams.
//!
//! The client is self-contained: it depends only on `reqwest` (blocking), serde
//! and the generated (or import-mapped) model types — never on axum. It reuses
//! the same [`Service`] IR the axum server emitter consumes, so the two stay in
//! lock-step through the shared operation-naming helpers.

use proc_macro2::Literal;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;

use crate::emit::doc_attr;
use crate::emit::emit_type;
use crate::emit::models::emit_struct;
use crate::error::Error;
use crate::error::Result;
use crate::ir::BodyKind;
use crate::ir::Cookies;
use crate::ir::Headers;
use crate::ir::Operation;
use crate::ir::RequestPayload;
use crate::ir::ResponseBody;
use crate::ir::ResponseCase;
use crate::ir::ResponseStatus;
use crate::ir::RustType;
use crate::ir::SecurityScheme;
use crate::ir::SecuritySchemeKind;
use crate::ir::Service;

/// The blocking `reqwest` client emitter.
pub struct ReqwestClient;

impl crate::emit::ClientEmitter for ReqwestClient {
    fn emit(&self, service: &Service) -> Result<Vec<TokenStream>> {
        return client_items(service);
    }
}

/// Build the error for an operation shape the client generator does not support.
fn unsupported(operation: &Operation, reason: &str) -> Error {
    return Error::UnsupportedOperation {
        method: operation.method.clone(),
        path: operation.path.clone(),
        reason: reason.to_owned(),
    };
}

/// Reject the operation shapes the client generator does not handle yet, so a
/// spec that uses them fails loudly rather than generating a client that silently
/// drops the body.
fn ensure_supported(operation: &Operation, schemes: &[SecurityScheme]) -> Result<()> {
    match &operation.request {
        Some(RequestPayload::Multipart(_)) => {
            return Err(unsupported(
                operation,
                "multipart/form-data request bodies are not supported by the client generator yet",
            ));
        }
        Some(RequestPayload::Negotiated(_)) => {
            return Err(unsupported(
                operation,
                "multi-content-type (negotiated) request bodies are not supported by the client generator yet",
            ));
        }
        Some(RequestPayload::Single(_)) | None => {}
    }
    for case in &operation.responses {
        match &case.body {
            Some(ResponseBody::Negotiated(_)) => {
                return Err(unsupported(
                    operation,
                    "multi-content-type (negotiated) response bodies are not supported by the client generator yet",
                ));
            }
            Some(ResponseBody::Single(body)) if body.kind == BodyKind::Form => {
                return Err(unsupported(
                    operation,
                    "form (`application/x-www-form-urlencoded`) response bodies are not supported by the client generator yet",
                ));
            }
            _ => {}
        }
    }
    for key in &operation.security {
        let scheme = schemes.iter().find(|scheme| return scheme.key == *key);
        if let Some(scheme) = scheme
            && let SecuritySchemeKind::Unsupported(reason) = &scheme.kind
        {
            return Err(unsupported(operation, reason));
        }
    }
    return Ok(());
}

/// Emit the client: the `ClientError` type, per-operation input/response types,
/// the `Client` struct, and its inherent `impl` with one method per operation.
fn client_items(service: &Service) -> Result<Vec<TokenStream>> {
    for operation in &service.operations {
        ensure_supported(operation, &service.security_schemes)?;
    }

    let mut items = vec![client_error()];
    for operation in &service.operations {
        if let Some(query) = &operation.query {
            items.push(emit_struct(query)?);
        }
        if let Some(headers) = &operation.headers {
            items.push(emit_headers_struct(headers)?);
        }
        if let Some(cookies) = &operation.cookies {
            items.push(emit_cookies_struct(cookies)?);
        }
        items.push(emit_response_enum(operation)?);
    }
    items.push(client_struct(&service.security_schemes));
    items.push(emit_client_impl(service)?);
    return Ok(items);
}

/// Emit the `ClientError` type shared by every client method.
fn client_error() -> TokenStream {
    return quote! {
        /// Errors returned by the generated client.
        #[derive(Debug)]
        pub enum ClientError {
            /// The request failed to send, or the response body failed to decode.
            Http(reqwest::Error),
            /// The server returned a status code the operation does not declare.
            UnexpectedStatus(reqwest::StatusCode),
        }

        impl std::fmt::Display for ClientError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    ClientError::Http(error) => return write!(f, "HTTP request failed: {error}"),
                    ClientError::UnexpectedStatus(status) => {
                        return write!(f, "unexpected response status: {status}");
                    }
                }
            }
        }

        impl std::error::Error for ClientError {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                    ClientError::Http(error) => return Some(error),
                    ClientError::UnexpectedStatus(_) => return None,
                }
            }
        }

        impl From<reqwest::Error> for ClientError {
            fn from(error: reqwest::Error) -> Self {
                return ClientError::Http(error);
            }
        }
    };
}

/// Emit the `Client` struct holding the base URL, blocking `reqwest` client, and
/// one optional credential field per configured security scheme.
fn client_struct(schemes: &[SecurityScheme]) -> TokenStream {
    let credentials = credential_fields(schemes);
    return quote! {
        /// A blocking HTTP client for the API.
        ///
        /// `base_url` is used as a prefix for every request path and should not
        /// carry a trailing slash (e.g. `https://api.example.com`).
        #[derive(Clone)]
        pub struct Client {
            base_url: String,
            http: reqwest::blocking::Client,
            #(#credentials,)*
        }
    };
}

/// Emit a plain input struct for an operation's header parameters. Unlike the
/// server's header struct this carries no extractor impl — the client reads the
/// fields to set request headers.
fn emit_headers_struct(headers: &Headers) -> Result<TokenStream> {
    let name = headers.name.to_token();
    let mut fields = Vec::with_capacity(headers.params.len());
    for param in &headers.params {
        let field = param.name.to_token();
        let ty = emit_type(&param.ty)?;
        let ty = if param.required {
            ty
        } else {
            quote! { Option<#ty> }
        };
        let doc = doc_attr(&param.doc);
        fields.push(quote! { #doc pub #field: #ty });
    }
    return Ok(quote! {
        pub struct #name {
            #(#fields),*
        }
    });
}

/// Emit a plain input struct for an operation's cookie parameters, mirroring
/// [`emit_headers_struct`].
fn emit_cookies_struct(cookies: &Cookies) -> Result<TokenStream> {
    let name = cookies.name.to_token();
    let mut fields = Vec::with_capacity(cookies.params.len());
    for param in &cookies.params {
        let field = param.name.to_token();
        let ty = emit_type(&param.ty)?;
        let ty = if param.required {
            ty
        } else {
            quote! { Option<#ty> }
        };
        let doc = doc_attr(&param.doc);
        fields.push(quote! { #doc pub #field: #ty });
    }
    return Ok(quote! {
        pub struct #name {
            #(#fields),*
        }
    });
}

/// Emit the response enum an operation's method returns.
///
/// The variant shapes mirror the server's response enum (a status field for
/// `default`/range responses, then the body, then declared headers) but the
/// status uses `reqwest::StatusCode` and header fields are always `Option<T>`:
/// a client cannot assume the server honoured a required-header contract.
fn emit_response_enum(operation: &Operation) -> Result<TokenStream> {
    let name = operation.response_enum.to_token();
    let doc = doc_attr(&operation.doc);
    let mut variants = Vec::with_capacity(operation.responses.len());
    for case in &operation.responses {
        let (variant_def, _) = response_case(&name, case)?;
        let case_doc = doc_attr(&case.doc);
        variants.push(quote! { #case_doc #variant_def });
    }
    return Ok(quote! {
        #doc
        pub enum #name {
            #(#variants),*
        }
    });
}

/// Emit the `impl Client` block: the constructors, the `with_<scheme>` credential
/// setters, then one method per operation.
fn emit_client_impl(service: &Service) -> Result<TokenStream> {
    let schemes = &service.security_schemes;
    let inits = credential_inits(schemes);
    let mut methods = Vec::with_capacity(service.operations.len() + schemes.len() + 2);
    methods.push(quote! {
        /// Build a client targeting `base_url` with a default blocking
        /// `reqwest::blocking::Client`.
        pub fn new(base_url: impl Into<String>) -> Result<Self, ClientError> {
            let http = reqwest::blocking::Client::builder().build()?;
            return Ok(Self { base_url: base_url.into(), http, #(#inits,)* });
        }
    });
    methods.push(quote! {
        /// Build a client targeting `base_url` with a caller-provided
        /// `reqwest::blocking::Client` (e.g. preconfigured with timeouts).
        pub fn with_client(base_url: impl Into<String>, http: reqwest::blocking::Client) -> Self {
            return Self { base_url: base_url.into(), http, #(#inits,)* };
        }
    });
    for setter in credential_setters(schemes) {
        methods.push(setter);
    }
    for operation in &service.operations {
        methods.push(emit_method(operation, schemes)?);
    }
    return Ok(quote! {
        impl Client {
            #(#methods)*
        }
    });
}

/// The credential fields added to the `Client` struct, one per supported scheme.
fn credential_fields(schemes: &[SecurityScheme]) -> Vec<TokenStream> {
    let mut fields = Vec::new();
    for scheme in schemes {
        let Some(ty) = credential_field_type(&scheme.kind) else {
            continue;
        };
        let field = scheme.field.to_token();
        let doc = doc_attr(&scheme.doc);
        fields.push(quote! { #doc #field: #ty });
    }
    return fields;
}

/// The `<field>: None` initializers the constructors use to start every
/// credential unset.
fn credential_inits(schemes: &[SecurityScheme]) -> Vec<TokenStream> {
    let mut inits = Vec::new();
    for scheme in schemes {
        if credential_field_type(&scheme.kind).is_none() {
            continue;
        }
        let field = scheme.field.to_token();
        inits.push(quote! { #field: None });
    }
    return inits;
}

/// The builder-style `with_<scheme>` setters, one per supported scheme.
fn credential_setters(schemes: &[SecurityScheme]) -> Vec<TokenStream> {
    let mut setters = Vec::new();
    for scheme in schemes {
        let field = scheme.field.to_token();
        let setter = format_ident!("with_{}", scheme.field.logical());
        let setter = match &scheme.kind {
            SecuritySchemeKind::HttpBasic => {
                let doc = format!(
                    " Set the username and password for the `{}` HTTP basic scheme.",
                    scheme.key
                );
                quote! {
                    #[doc = #doc]
                    pub fn #setter(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
                        self.#field = Some((username.into(), password.into()));
                        return self;
                    }
                }
            }
            SecuritySchemeKind::HttpBearer
            | SecuritySchemeKind::ApiKeyHeader(_)
            | SecuritySchemeKind::ApiKeyQuery(_)
            | SecuritySchemeKind::ApiKeyCookie(_) => {
                let doc = format!(" Set the credential for the `{}` security scheme.", scheme.key);
                quote! {
                    #[doc = #doc]
                    pub fn #setter(mut self, credential: impl Into<String>) -> Self {
                        self.#field = Some(credential.into());
                        return self;
                    }
                }
            }
            SecuritySchemeKind::Unsupported(_) => continue,
        };
        setters.push(setter);
    }
    return setters;
}

/// The stored credential type for a scheme, or `None` for schemes the client
/// cannot carry (which are rejected before emit for any operation that uses one).
fn credential_field_type(kind: &SecuritySchemeKind) -> Option<TokenStream> {
    return match kind {
        SecuritySchemeKind::HttpBasic => Some(quote! { Option<(String, String)> }),
        SecuritySchemeKind::HttpBearer
        | SecuritySchemeKind::ApiKeyHeader(_)
        | SecuritySchemeKind::ApiKeyQuery(_)
        | SecuritySchemeKind::ApiKeyCookie(_) => Some(quote! { Option<String> }),
        SecuritySchemeKind::Unsupported(_) => None,
    };
}

/// Emit one operation method: build the request from the typed inputs, send it,
/// and decode the response into the operation's typed response enum.
fn emit_method(operation: &Operation, schemes: &[SecurityScheme]) -> Result<TokenStream> {
    let name = operation.name.to_token();
    let doc = doc_attr(&operation.doc);
    let response = operation.response_enum.to_token();
    let args = method_args(operation)?;

    let url = url_expr(operation);
    let method = format_ident!("{}", operation.method.to_uppercase());
    let builder = quote! { self.http.request(reqwest::Method::#method, url) };

    let mut mutations = Vec::new();
    mutations.extend(query_mutations(operation));
    mutations.extend(header_mutations(operation));
    mutations.extend(cookie_mutations(operation));
    mutations.extend(body_mutations(operation)?);
    mutations.extend(auth_mutations(operation, schemes));

    let send = if mutations.is_empty() {
        quote! { let response = #builder.send()?; }
    } else {
        quote! {
            let mut request = #builder;
            #(#mutations)*
            let response = request.send()?;
        }
    };

    let decode = decode_response(operation)?;

    return Ok(quote! {
        #doc
        pub fn #name(&self, #(#args),*) -> Result<#response, ClientError> {
            let url = #url;
            #send
            let status = response.status();
            #decode
        }
    });
}

/// The typed method arguments: path parameters, query struct, header struct,
/// cookie struct, then the request body (a single supported content type).
fn method_args(operation: &Operation) -> Result<Vec<TokenStream>> {
    let mut args = Vec::new();
    for param in &operation.path_params {
        let name = param.name.to_token();
        let ty = emit_type(&param.ty)?;
        args.push(quote! { #name: #ty });
    }
    if let Some(query) = &operation.query {
        let ty = query.name.to_token();
        args.push(quote! { query: #ty });
    }
    if let Some(headers) = &operation.headers {
        let ty = headers.name.to_token();
        args.push(quote! { headers: #ty });
    }
    if let Some(cookies) = &operation.cookies {
        let ty = cookies.name.to_token();
        args.push(quote! { cookies: #ty });
    }
    if let Some(RequestPayload::Single(body)) = &operation.request {
        let ty = emit_type(&body.ty)?;
        args.push(quote! { body: #ty });
    }
    return Ok(args);
}

/// Build the `format!` expression producing the request URL: the base URL
/// followed by the path template with each `{placeholder}` replaced by the
/// corresponding path argument, in path order.
fn url_expr(operation: &Operation) -> TokenStream {
    let mut literal = String::from("{}");
    let mut args: Vec<TokenStream> = vec![quote! { self.base_url }];
    let mut params = operation.path_params.iter();
    let mut rest = operation.path.as_str();
    while let Some(open) = rest.find('{') {
        literal.push_str(&rest[..open]);
        literal.push_str("{}");
        let after = match rest[open..].find('}') {
            Some(close) => open + close + 1,
            None => rest.len(),
        };
        rest = &rest[after..];
        if let Some(param) = params.next() {
            let name = param.name.to_token();
            args.push(quote! { #name });
        }
    }
    literal.push_str(rest);
    let literal = Literal::string(&literal);
    return quote! { format!(#literal, #(#args),*) };
}

/// The request-builder mutations that append query parameters. Arrays are sent
/// as repeated keys (OpenAPI's default `explode: true`), matching the server's
/// `axum_extra` query extractor; `serde_urlencoded` cannot serialize sequences,
/// so each pair is appended individually.
fn query_mutations(operation: &Operation) -> Vec<TokenStream> {
    let Some(query) = &operation.query else {
        return Vec::new();
    };
    let mut mutations = Vec::with_capacity(query.fields.len());
    for field in &query.fields {
        let field_name = field.name.to_token();
        let wire = field
            .rename
            .clone()
            .unwrap_or_else(|| return field.name.logical().to_owned());
        let wire = Literal::string(&wire);
        let mutation = match &field.ty {
            RustType::Option(inner) if matches!(**inner, RustType::Vec(_)) => quote! {
                if let Some(items) = &query.#field_name {
                    for item in items {
                        request = request.query(&[(#wire, item.to_string())]);
                    }
                }
            },
            RustType::Option(_) => quote! {
                if let Some(value) = &query.#field_name {
                    request = request.query(&[(#wire, value.to_string())]);
                }
            },
            RustType::Vec(_) => quote! {
                for item in &query.#field_name {
                    request = request.query(&[(#wire, item.to_string())]);
                }
            },
            _ => quote! {
                request = request.query(&[(#wire, query.#field_name.to_string())]);
            },
        };
        mutations.push(mutation);
    }
    return mutations;
}

/// The request-builder mutations that set request headers from the header struct.
fn header_mutations(operation: &Operation) -> Vec<TokenStream> {
    let Some(headers) = &operation.headers else {
        return Vec::new();
    };
    let mut mutations = Vec::with_capacity(headers.params.len());
    for param in &headers.params {
        let field = param.name.to_token();
        let header_name = Literal::string(&param.header_name);
        let mutation = if param.required {
            quote! { request = request.header(#header_name, headers.#field.to_string()); }
        } else {
            quote! {
                if let Some(value) = &headers.#field {
                    request = request.header(#header_name, value.to_string());
                }
            }
        };
        mutations.push(mutation);
    }
    return mutations;
}

/// The request-builder mutations that collect cookie parameters into a single
/// `Cookie` header.
fn cookie_mutations(operation: &Operation) -> Vec<TokenStream> {
    let Some(cookies) = &operation.cookies else {
        return Vec::new();
    };
    let entries: Vec<TokenStream> = cookies
        .params
        .iter()
        .map(|param| {
            let field = param.name.to_token();
            if param.required {
                let fmt = Literal::string(&format!("{}={{}}", param.cookie_name));
                return quote! { Some(format!(#fmt, cookies.#field)) };
            }
            let fmt = Literal::string(&format!("{}={{value}}", param.cookie_name));
            return quote! { cookies.#field.as_ref().map(|value| format!(#fmt)) };
        })
        .collect();
    return vec![quote! {
        let cookie_pairs: Vec<String> = [#(#entries),*]
            .into_iter()
            .flatten()
            .collect();
        if !cookie_pairs.is_empty() {
            request = request.header(reqwest::header::COOKIE, cookie_pairs.join("; "));
        }
    }];
}

/// The request-builder mutation that attaches the request body, if any.
fn body_mutations(operation: &Operation) -> Result<Vec<TokenStream>> {
    let mutation = match &operation.request {
        Some(RequestPayload::Single(body)) => match body.kind {
            BodyKind::Json => quote! { request = request.json(&body); },
            BodyKind::Form => quote! { request = request.form(&body); },
            BodyKind::Text => quote! {
                request = request.header(reqwest::header::CONTENT_TYPE, "text/plain").body(body);
            },
            BodyKind::Multipart => {
                return Err(unsupported(operation, "multipart request bodies are rejected earlier"));
            }
        },
        Some(_) => {
            return Err(unsupported(operation, "unsupported request body is rejected earlier"));
        }
        None => return Ok(Vec::new()),
    };
    return Ok(vec![mutation]);
}

/// The request-builder mutations that apply the operation's security credentials.
///
/// Each configured credential is optional, so an unset scheme simply sends no
/// auth. Operations that require an [`SecuritySchemeKind::Unsupported`] scheme
/// are rejected in [`ensure_supported`] before this runs.
fn auth_mutations(operation: &Operation, schemes: &[SecurityScheme]) -> Vec<TokenStream> {
    let mut mutations = Vec::new();
    for key in &operation.security {
        let Some(scheme) = schemes.iter().find(|scheme| return scheme.key == *key) else {
            continue;
        };
        let field = scheme.field.to_token();
        let mutation = match &scheme.kind {
            SecuritySchemeKind::HttpBearer => quote! {
                if let Some(token) = &self.#field {
                    request = request.bearer_auth(token);
                }
            },
            SecuritySchemeKind::HttpBasic => quote! {
                if let Some((username, password)) = &self.#field {
                    request = request.basic_auth(username, Some(password));
                }
            },
            SecuritySchemeKind::ApiKeyHeader(name) => {
                let header = Literal::string(name);
                quote! {
                    if let Some(value) = &self.#field {
                        request = request.header(#header, value.as_str());
                    }
                }
            }
            SecuritySchemeKind::ApiKeyQuery(name) => {
                let param = Literal::string(name);
                quote! {
                    if let Some(value) = &self.#field {
                        request = request.query(&[(#param, value.as_str())]);
                    }
                }
            }
            SecuritySchemeKind::ApiKeyCookie(name) => {
                let fmt = Literal::string(&format!("{name}={{value}}"));
                quote! {
                    if let Some(value) = &self.#field {
                        request = request.header(reqwest::header::COOKIE, format!(#fmt));
                    }
                }
            }
            SecuritySchemeKind::Unsupported(_) => continue,
        };
        mutations.push(mutation);
    }
    return mutations;
}
///
/// Fixed status codes are tried first (most specific), then ranges (`5XX`), then
/// the `default` catch-all; an undeclared status yields `UnexpectedStatus`.
fn decode_response(operation: &Operation) -> Result<TokenStream> {
    let name = operation.response_enum.to_token();
    let mut fixed = Vec::new();
    let mut ranges = Vec::new();
    let mut default = None;
    for case in &operation.responses {
        let (_, build) = response_case(&name, case)?;
        match &case.status {
            ResponseStatus::Fixed(code) => {
                let code = Literal::u16_unsuffixed(*code);
                fixed.push(quote! {
                    if status.as_u16() == #code {
                        #build
                    }
                });
            }
            ResponseStatus::Range(digit) => {
                let digit = Literal::u16_unsuffixed(u16::from(*digit));
                ranges.push(quote! {
                    if status.as_u16() / 100 == #digit {
                        #build
                    }
                });
            }
            ResponseStatus::Default => {
                default = Some(build);
            }
        }
    }
    let tail = match default {
        Some(build) => build,
        None => quote! { return Err(ClientError::UnexpectedStatus(status)); },
    };
    return Ok(quote! {
        #(#fixed)*
        #(#ranges)*
        #tail
    });
}

/// Emit both the enum variant definition and the decode block for one response
/// case, keeping the two in agreement. The decode block reads the declared
/// headers, decodes the body, and returns the constructed variant.
fn response_case(name: &proc_macro2::Ident, case: &ResponseCase) -> Result<(TokenStream, TokenStream)> {
    let variant = case.variant.to_token();
    let dynamic = !matches!(case.status, ResponseStatus::Fixed(_));
    let body_ty = match &case.body {
        Some(ResponseBody::Single(body)) => Some(emit_type(&body.ty)?),
        _ => None,
    };
    let body_kind = match &case.body {
        Some(ResponseBody::Single(body)) => Some(body.kind),
        _ => None,
    };
    let decode_body = body_decode(body_kind, &body_ty);

    if case.headers.is_empty() {
        let variant_def = match (&body_ty, dynamic) {
            (None, false) => quote! { #variant },
            (None, true) => quote! { #variant(reqwest::StatusCode) },
            (Some(ty), false) => quote! { #variant(#ty) },
            (Some(ty), true) => quote! { #variant(reqwest::StatusCode, #ty) },
        };
        let construct = match (&body_ty, dynamic) {
            (None, false) => quote! { #name::#variant },
            (None, true) => quote! { #name::#variant(status) },
            (Some(_), false) => quote! { #name::#variant(body) },
            (Some(_), true) => quote! { #name::#variant(status, body) },
        };
        let build = quote! {
            #decode_body
            return Ok(#construct);
        };
        return Ok((variant_def, build));
    }

    let mut field_defs = Vec::new();
    let mut construct_fields = Vec::new();
    if dynamic {
        field_defs.push(quote! { status: reqwest::StatusCode });
        construct_fields.push(quote! { status });
    }
    if let Some(ty) = &body_ty {
        field_defs.push(quote! { body: #ty });
        construct_fields.push(quote! { body });
    }
    let mut header_reads = Vec::with_capacity(case.headers.len());
    for header in &case.headers {
        let field = header.name.to_token();
        let ty = emit_type(&header.ty)?;
        let header_name = Literal::string(&header.header_name);
        let doc = doc_attr(&header.doc);
        field_defs.push(quote! { #doc #field: Option<#ty> });
        header_reads.push(quote! {
            let #field: Option<#ty> = response
                .headers()
                .get(#header_name)
                .and_then(|value| return value.to_str().ok())
                .and_then(|value| return value.parse().ok());
        });
        construct_fields.push(quote! { #field });
    }
    let variant_def = quote! { #variant { #(#field_defs),* } };
    let build = quote! {
        #(#header_reads)*
        #decode_body
        return Ok(#name::#variant { #(#construct_fields),* });
    };
    return Ok((variant_def, build));
}

/// The statement that decodes the response body into `body`, or nothing when the
/// response declares no body.
fn body_decode(kind: Option<BodyKind>, ty: &Option<TokenStream>) -> TokenStream {
    return match (kind, ty) {
        (Some(BodyKind::Json), Some(ty)) => quote! { let body: #ty = response.json()?; },
        (Some(BodyKind::Text), _) => quote! { let body = response.text()?; },
        _ => quote! {},
    };
}
