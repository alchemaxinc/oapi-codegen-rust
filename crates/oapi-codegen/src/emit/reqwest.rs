//! Emitting the blocking `reqwest` client as token streams.
//!
//! The client is self-contained: it depends only on `reqwest` (blocking), serde,
//! `percent-encoding` (to escape path parameters) and the generated (or
//! import-mapped) model types — never on axum. It reuses the same [`Service`] IR
//! the axum server emitter consumes, so the two stay in lock-step through the
//! shared operation-naming helpers.

use proc_macro2::Literal;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;

use crate::emit::doc_attr;
use crate::emit::emit_type;
use crate::error::Error;
use crate::error::Result;
use crate::ir::BodyKind;
use crate::ir::Multipart;
use crate::ir::NegotiatedBody;
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

/// Name of the emitted client struct. reserved at the crate root so a component
/// schema cannot collide with it (see [`crate::emit::reserved_type_names`]).
pub(crate) const CLIENT_STRUCT_NAME: &str = "Client";

/// Name of the emitted client error enum. reserved at the crate root so a
/// component schema cannot collide with it.
pub(crate) const CLIENT_ERROR_NAME: &str = "ClientError";

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
    for key in &operation.security {
        match schemes.iter().find(|scheme| return scheme.key == *key) {
            None => {
                return Err(unsupported(
                    operation,
                    &format!("requires security scheme `{key}`, which is not declared in `components.securitySchemes`"),
                ));
            }
            Some(scheme) => {
                if let SecuritySchemeKind::Unsupported(reason) = &scheme.kind {
                    return Err(unsupported(operation, reason));
                }
            }
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
    if service
        .operations
        .iter()
        .any(|operation| return !operation.path_params.is_empty())
    {
        items.push(path_param_encode_set());
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
            /// The `reqwest` request failed to send or complete, including any
            /// body decoding `reqwest` performs internally (such as JSON).
            Http(reqwest::Error),
            /// The server returned a status code the operation does not declare.
            UnexpectedStatus(reqwest::StatusCode),
            /// The response `Content-Type` matched none of the representations the
            /// operation declares for its status.
            UnexpectedContentType(String),
            /// The response cannot be decoded: a body that failed to
            /// deserialize (for example malformed form-urlencoded content), or a
            /// required response header that was missing or unparsable.
            Decode(String),
        }

        impl std::fmt::Display for ClientError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    ClientError::Http(error) => return write!(f, "HTTP request failed: {error}"),
                    ClientError::UnexpectedStatus(status) => {
                        return write!(f, "unexpected response status: {status}");
                    }
                    ClientError::UnexpectedContentType(content_type) => {
                        return write!(f, "unexpected response content type: {content_type}");
                    }
                    ClientError::Decode(message) => {
                        return write!(f, "failed to decode response: {message}");
                    }
                }
            }
        }

        impl std::error::Error for ClientError {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                    ClientError::Http(error) => return Some(error),
                    ClientError::UnexpectedStatus(_)
                    | ClientError::UnexpectedContentType(_)
                    | ClientError::Decode(_) => return None,
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

/// Emit the module-level `AsciiSet` used to percent-encode path parameters.
///
/// Every character outside the RFC 3986 unreserved set (`A-Za-z0-9-._~`) is
/// encoded so a parameter value containing `/`, `?`, `#`, `%` or whitespace can
/// never break out of its URL path segment.
fn path_param_encode_set() -> TokenStream {
    return quote! {
        const PATH_PARAM_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'.')
            .remove(b'_')
            .remove(b'~');
    };
}

/// Emit the `Client` struct holding the base URL, blocking `reqwest` client, and
/// one optional credential field per configured security scheme.
fn client_struct(schemes: &[SecurityScheme]) -> TokenStream {
    let credentials = credential_fields(schemes);
    return quote! {
        /// A blocking HTTP client for the API.
        ///
        /// `base_url` is used as a prefix for every request path and must not
        /// carry a trailing slash (for example `https://api.example.com`).
        #[derive(Debug, Clone)]
        pub struct Client {
            base_url: String,
            http: reqwest::blocking::Client,
            #(#credentials,)*
        }
    };
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
        /// `reqwest::blocking::Client` (for example preconfigured with timeouts).
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
    mutations.extend(cookie_mutations(operation, schemes));
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
    match &operation.request {
        Some(RequestPayload::Single(body)) => {
            let ty = emit_type(&body.ty)?;
            args.push(quote! { body: #ty });
        }
        Some(RequestPayload::Multipart(multipart)) => {
            let ty = multipart.name.to_token();
            args.push(quote! { body: #ty });
        }
        Some(RequestPayload::Negotiated(request)) => {
            let ty = request.name.to_token();
            args.push(quote! { body: #ty });
        }
        None => {}
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
            let encoded = if matches!(param.ty, RustType::String) {
                quote! { percent_encoding::utf8_percent_encode(#name.as_str(), PATH_PARAM_ENCODE_SET) }
            } else {
                quote! { percent_encoding::utf8_percent_encode(&#name.to_string(), PATH_PARAM_ENCODE_SET) }
            };
            args.push(encoded);
        }
    }
    literal.push_str(rest);
    let literal = Literal::string(&literal);
    return quote! { format!(#literal, #(#args),*) };
}

/// The request-builder mutations that append query parameters. Arrays are sent
/// as repeated keys (OpenAPI's default `explode: true`), matching the server's
/// `axum_extra` query extractor. `serde_urlencoded` cannot serialize sequences,
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

/// The request-builder mutation that collects cookie parameters and any
/// cookie-carried API-key credentials into a single `Cookie` header.
///
/// Auth cookies are folded in here (rather than emitted separately) so an
/// operation with both cookie parameters and a cookie credential sends one
/// `Cookie` header, per RFC 6265.
fn cookie_mutations(operation: &Operation, schemes: &[SecurityScheme]) -> Vec<TokenStream> {
    let mut entries: Vec<TokenStream> = Vec::new();
    if let Some(cookies) = &operation.cookies {
        for param in &cookies.params {
            let field = param.name.to_token();
            if param.required {
                let fmt = Literal::string(&format!("{}={{}}", param.cookie_name));
                entries.push(quote! { Some(format!(#fmt, cookies.#field)) });
            } else {
                let fmt = Literal::string(&format!("{}={{value}}", param.cookie_name));
                entries.push(quote! { cookies.#field.as_ref().map(|value| format!(#fmt)) });
            }
        }
    }
    for key in &operation.security {
        let Some(scheme) = schemes.iter().find(|scheme| return scheme.key == *key) else {
            continue;
        };
        if let SecuritySchemeKind::ApiKeyCookie(name) = &scheme.kind {
            let field = scheme.field.to_token();
            let fmt = Literal::string(&format!("{name}={{value}}"));
            entries.push(quote! { self.#field.as_ref().map(|value| format!(#fmt)) });
        }
    }
    if entries.is_empty() {
        return Vec::new();
    }
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
        Some(RequestPayload::Multipart(multipart)) => return Ok(multipart_form_mutations(multipart)),
        Some(RequestPayload::Negotiated(request)) => return Ok(negotiated_body_mutations(request)),
        None => return Ok(Vec::new()),
    };
    return Ok(vec![mutation]);
}

/// Build the request from a negotiated (multi-content-type) body enum: match on
/// the caller-selected representation and apply the matching `reqwest` builder,
/// mirroring the single-body content-type handling per variant.
fn negotiated_body_mutations(request: &NegotiatedBody) -> Vec<TokenStream> {
    let name = request.name.to_token();
    let mut arms = Vec::with_capacity(request.variants.len());
    for variant in &request.variants {
        let ident = variant.variant.to_token();
        let apply = match variant.body.kind {
            BodyKind::Json => quote! { request = request.json(&value); },
            BodyKind::Form => quote! { request = request.form(&value); },
            BodyKind::Text => quote! {
                request = request.header(reqwest::header::CONTENT_TYPE, "text/plain").body(value);
            },
            BodyKind::Multipart => quote! {},
        };
        arms.push(quote! { #name::#ident(value) => { #apply } });
    }
    return vec![quote! {
        match body {
            #(#arms)*
        }
    }];
}

/// Build the `reqwest::blocking::multipart::Form` from the typed `<Op>Multipart`
/// body: scalar parts are sent as text (their `Display`), binary/file parts as
/// raw bytes with a filename, and optional parts are only attached when present.
fn multipart_form_mutations(multipart: &Multipart) -> Vec<TokenStream> {
    let mut mutations = vec![quote! {
        let mut form = reqwest::blocking::multipart::Form::new();
    }];
    for field in &multipart.fields {
        let ident = field.rust_name.to_token();
        let wire = Literal::string(&field.wire_name);
        let mutation = match (field.is_file, field.optional) {
            (true, false) => quote! {
                form = form.part(#wire, reqwest::blocking::multipart::Part::bytes(body.#ident).file_name(#wire));
            },
            (true, true) => quote! {
                if let Some(value) = body.#ident {
                    form = form.part(#wire, reqwest::blocking::multipart::Part::bytes(value).file_name(#wire));
                }
            },
            (false, false) => quote! {
                form = form.text(#wire, body.#ident.to_string());
            },
            (false, true) => quote! {
                if let Some(value) = &body.#ident {
                    form = form.text(#wire, value.to_string());
                }
            },
        };
        mutations.push(mutation);
    }
    mutations.push(quote! { request = request.multipart(form); });
    return mutations;
}

/// The request-builder mutations that apply the operation's security credentials.
///
/// Each configured credential is optional, so an unset scheme  sends no
/// auth. Cookie-carried API keys are applied by [`cookie_mutations`] (folded into
/// the single `Cookie` header), so they are skipped here. Operations that require
/// an unresolved or [`SecuritySchemeKind::Unsupported`] scheme are rejected in
/// [`ensure_supported`] before this runs.
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
            SecuritySchemeKind::ApiKeyCookie(_) => continue,
            SecuritySchemeKind::Unsupported(_) => continue,
        };
        mutations.push(mutation);
    }
    return mutations;
}

/// Emit the status-code dispatch that decodes the response into a typed variant.
///
/// Fixed status codes are tried first (most specific), then ranges (`5XX`), then
/// the `default` catch-all. An undeclared status yields `UnexpectedStatus`.
fn decode_response(operation: &Operation) -> Result<TokenStream> {
    let name = operation.response_enum.to_token();
    let mut fixed = Vec::new();
    let mut ranges = Vec::new();
    let mut default = None;
    for case in &operation.responses {
        let build = response_case_build(&name, case)?;
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

/// Emit the decode block for one response case: read the declared headers,
/// decode the body, and return the constructed shared response-enum variant
/// (defined by [`crate::emit::operation`]).
fn response_case_build(name: &proc_macro2::Ident, case: &ResponseCase) -> Result<TokenStream> {
    let variant = case.variant.to_token();
    let dynamic = !matches!(case.status, ResponseStatus::Fixed(_));
    let body_ty = response_body_type(&case.body)?;
    let decode_body = body_decode(&case.body)?;

    if case.headers.is_empty() {
        let construct = match (&body_ty, dynamic) {
            (None, false) => quote! { #name::#variant },
            (None, true) => quote! { #name::#variant(status) },
            (Some(_), false) => quote! { #name::#variant(body) },
            (Some(_), true) => quote! { #name::#variant(status, body) },
        };
        return Ok(quote! {
            #decode_body
            return Ok(#construct);
        });
    }

    let mut construct_fields = Vec::new();
    if dynamic {
        construct_fields.push(quote! { status });
    }
    if body_ty.is_some() {
        construct_fields.push(quote! { body });
    }
    let mut header_reads = Vec::with_capacity(case.headers.len());
    for header in &case.headers {
        let field = header.name.to_token();
        let ty = emit_type(&header.ty)?;
        let header_name = Literal::string(&header.header_name);
        let read = quote! {
            response
                .headers()
                .get(#header_name)
                .and_then(|value| return value.to_str().ok())
                .and_then(|value| return value.parse().ok())
        };
        if header.required {
            let missing = format!("missing or invalid required response header `{}`", header.header_name);
            header_reads.push(quote! {
                let #field: #ty = #read
                    .ok_or_else(|| return ClientError::Decode(#missing.to_owned()))?;
            });
        } else {
            header_reads.push(quote! {
                let #field: Option<#ty> = #read;
            });
        }
        construct_fields.push(quote! { #field });
    }
    return Ok(quote! {
        #(#header_reads)*
        #decode_body
        return Ok(#name::#variant { #(#construct_fields),* });
    });
}

/// The Rust type a response variant carries for its body, if any: the decoded
/// type for a single-content body, or the negotiated enum type for a
/// multi-content body.
fn response_body_type(body: &Option<ResponseBody>) -> Result<Option<TokenStream>> {
    return Ok(match body {
        Some(ResponseBody::Single(body)) => Some(emit_type(&body.ty)?),
        Some(ResponseBody::Negotiated(negotiated)) => {
            let ident = negotiated.name.to_token();
            Some(quote! { #ident })
        }
        None => None,
    });
}

/// The statement(s) that decode the response body into `body`, or nothing when
/// the response declares no body.
///
/// JSON and text use `reqwest`'s built-in decoders. form bodies are decoded with
/// `serde_urlencoded` (`reqwest` has no form decoder). negotiated bodies dispatch
/// on the response `Content-Type` (see [`negotiated_response_decode`]).
fn body_decode(body: &Option<ResponseBody>) -> Result<TokenStream> {
    return Ok(match body {
        Some(ResponseBody::Single(body)) => {
            let ty = emit_type(&body.ty)?;
            match body.kind {
                BodyKind::Json => quote! { let body: #ty = response.json()?; },
                BodyKind::Text => quote! { let body = response.text()?; },
                BodyKind::Form => quote! {
                    let text = response.text()?;
                    let body: #ty = serde_urlencoded::from_str(&text)
                        .map_err(|error| return ClientError::Decode(error.to_string()))?;
                },
                BodyKind::Multipart => quote! {},
            }
        }
        Some(ResponseBody::Negotiated(negotiated)) => negotiated_response_decode(negotiated),
        None => quote! {},
    });
}

/// Decode a negotiated response body: read the response `Content-Type` and pick
/// the matching representation, wrapping the decoded value in the negotiated
/// enum. An unrecognised content type yields [`ClientError::UnexpectedContentType`].
fn negotiated_response_decode(negotiated: &NegotiatedBody) -> TokenStream {
    let name = negotiated.name.to_token();
    let mut arms = Vec::with_capacity(negotiated.variants.len());
    for variant in &negotiated.variants {
        let ident = variant.variant.to_token();
        let test = response_content_type_test(variant.body.kind);
        let decode = match variant.body.kind {
            BodyKind::Json => quote! { #name::#ident(response.json()?) },
            BodyKind::Text => quote! { #name::#ident(response.text()?) },
            BodyKind::Form => quote! {
                #name::#ident({
                    let text = response.text()?;
                    serde_urlencoded::from_str(&text)
                        .map_err(|error| return ClientError::Decode(error.to_string()))?
                })
            },
            BodyKind::Multipart => quote! {},
        };
        arms.push(quote! { if #test { #decode } else });
    }
    return quote! {
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| return value.to_str().ok())
            .map(|value| return value.split(';').next().unwrap_or(value).trim().to_ascii_lowercase())
            .unwrap_or_default();
        let body = #(#arms)* {
            return Err(ClientError::UnexpectedContentType(content_type));
        };
    };
}

/// The boolean test matching a response `Content-Type` (lower-cased, parameters
/// stripped) to a body kind — the client-side mirror of the server extractor's
/// content-type dispatch.
fn response_content_type_test(kind: BodyKind) -> TokenStream {
    return match kind {
        BodyKind::Json => quote! { content_type == "application/json" || content_type.ends_with("+json") },
        BodyKind::Form => quote! { content_type == "application/x-www-form-urlencoded" },
        BodyKind::Text => quote! { content_type == "text/plain" },
        BodyKind::Multipart => quote! { false },
    };
}
