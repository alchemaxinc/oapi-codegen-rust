//! Emitting the axum server interface as token streams.

use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;

use crate::emit::doc_attr;
use crate::emit::doc_lines;
use crate::emit::emit_type;
use crate::error::Result;
use crate::ir::BodyKind;
use crate::ir::BodyVariant;
use crate::ir::CookieParam;
use crate::ir::Cookies;
use crate::ir::HeaderParam;
use crate::ir::Headers;
use crate::ir::Multipart;
use crate::ir::MultipartField;
use crate::ir::NegotiatedBody;
use crate::ir::Operation;
use crate::ir::RequestPayload;
use crate::ir::ResponseBody;
use crate::ir::ResponseCase;
use crate::ir::ResponseHeader;
use crate::ir::ResponseStatus;
use crate::ir::RustType;
use crate::ir::SecurityScheme;
use crate::ir::SecuritySchemeKind;
use crate::ir::Service;
use crate::naming::operations::axum_handler_name;

/// The axum server-interface emitter.
pub struct AxumServer;

/// Name of the emitted server trait. reserved at the crate root so a component
/// schema cannot collide with it (see [`crate::emit::reserved_type_names`]).
pub(crate) const API_TRAIT_NAME: &str = "Api";

impl crate::emit::ServerEmitter for AxumServer {
    fn emit(&self, service: &Service) -> Result<Vec<TokenStream>> {
        return Ok(server_items(service, HandlerVisibility::Private)?.into_flat());
    }
}

/// How widely an emitted handler function is visible.
///
/// A flat file keeps the handler private, because the router that names it sits
/// beside it. A package puts the two in different modules, so the handler has to
/// reach its parent.
#[derive(Debug, Clone, Copy)]
pub(crate) enum HandlerVisibility {
    /// No visibility keyword: the handler is reachable within its own module.
    Private,
    /// `pub(super)`: the handler is reachable from the module holding the router.
    Parent,
}

impl HandlerVisibility {
    /// The visibility tokens to place before `async fn`.
    fn to_tokens(self) -> TokenStream {
        return match self {
            HandlerVisibility::Private => quote! {},
            HandlerVisibility::Parent => quote! { pub(super) },
        };
    }
}

/// The axum items one operation contributes, kept apart so a package can put
/// them in that operation's own file.
pub(crate) struct ServerOperationItems {
    /// The `FromRequestParts`/`FromRequest` impls for the operation's inputs.
    pub extractors: Vec<TokenStream>,
    /// The `IntoResponse` impl for the operation's response enum.
    pub into_response: TokenStream,
    /// The handler function the router dispatches to.
    pub handler: TokenStream,
}

/// The axum server interface, split into the items shared by every operation and
/// the items each operation contributes.
pub(crate) struct ServerItems {
    /// One entry per operation, in `service.operations` order.
    pub operations: Vec<ServerOperationItems>,
    /// The `Api` trait.
    pub api_trait: TokenStream,
    /// The `router` builder.
    pub router: TokenStream,
}

impl ServerItems {
    /// Flatten into the single-file item order: every extractor, the trait, every
    /// `IntoResponse` impl, the router, then every handler.
    pub fn into_flat(self) -> Vec<TokenStream> {
        let mut items = Vec::new();
        for operation in &self.operations {
            items.extend(operation.extractors.iter().cloned());
        }
        items.push(self.api_trait);
        for operation in &self.operations {
            items.push(operation.into_response.clone());
        }
        items.push(self.router);
        for operation in self.operations {
            items.push(operation.handler);
        }
        return items;
    }
}

/// The handler extractor pattern + type for a request body of the given kind.
///
/// Multipart bodies are emitted by [`emit_multipart`] and wired in by
/// [`emit_handler`] directly, so they never reach this helper.
fn body_extractor(kind: crate::ir::BodyKind, ty: &TokenStream) -> TokenStream {
    return match kind {
        crate::ir::BodyKind::Json => quote! { axum::Json(body): axum::Json<#ty> },
        crate::ir::BodyKind::Text => quote! { body: String },
        crate::ir::BodyKind::Form => quote! { axum::Form(body): axum::Form<#ty> },
        crate::ir::BodyKind::Multipart => {
            unreachable!("multipart bodies are emitted via emit_multipart, not body_extractor")
        }
    };
}

/// The response tuple term that renders a body of the given kind.
///
/// Multipart is request-only (`RESPONSE_BODY_PRIORITY` excludes it), so a
/// response body never carries [`crate::ir::BodyKind::Multipart`].
fn response_body_term(kind: crate::ir::BodyKind) -> TokenStream {
    return match kind {
        crate::ir::BodyKind::Json => quote! { axum::Json(body) },
        crate::ir::BodyKind::Text => quote! { body },
        crate::ir::BodyKind::Form => quote! { axum::Form(body) },
        crate::ir::BodyKind::Multipart => {
            unreachable!("multipart is request-only and never appears in a response body")
        }
    };
}

/// The `IntoResponse` match arms that render each representation of a negotiated
/// response body, binding the inner value to `body`. `status` is the status
/// expression (a `STATUS` constant or a bound `status`) placed first in the
/// response tuple. when `with_headers` is set, the in-scope `header_map` is
/// inserted between the status and the body wrapper.
fn negotiated_response_arms(body: &NegotiatedBody, status: &TokenStream, with_headers: bool) -> Vec<TokenStream> {
    let enum_name = body.name.to_token();
    return body
        .variants
        .iter()
        .map(|variant| {
            let ident = variant.variant.to_token();
            let term = response_body_term(variant.body.kind);
            let tuple = if with_headers {
                quote! { (#status, header_map, #term) }
            } else {
                quote! { (#status, #term) }
            };
            return quote! {
                #enum_name::#ident(body) => #tuple.into_response(),
            };
        })
        .collect();
}

/// Emit the axum server interface: the `Api` trait, per-operation `IntoResponse`
/// enums, the `Router` builder, and the internal handler functions.
pub(crate) fn server_items(service: &Service, handler_visibility: HandlerVisibility) -> Result<ServerItems> {
    let mut operations = Vec::with_capacity(service.operations.len());
    for operation in &service.operations {
        let mut extractors = Vec::new();
        if let Some(headers) = &operation.headers {
            extractors.push(emit_headers_extractor(headers)?);
        }
        if let Some(cookies) = &operation.cookies {
            extractors.push(emit_cookies_extractor(cookies)?);
        }
        match &operation.request {
            Some(RequestPayload::Multipart(multipart)) => extractors.push(emit_multipart_extractor(multipart)?),
            Some(RequestPayload::Negotiated(request)) => extractors.push(emit_request_body_extractor(request)?),
            Some(RequestPayload::Single(_)) | None => {}
        }
        operations.push(ServerOperationItems {
            extractors,
            into_response: emit_into_response(operation)?,
            handler: emit_handler(operation, handler_visibility)?,
        });
    }
    return Ok(ServerItems {
        operations,
        api_trait: emit_trait(service)?,
        router: emit_router(service),
    });
}

/// Emit the `Api` trait, one async method per operation.
fn emit_trait(service: &Service) -> Result<TokenStream> {
    let mut methods = Vec::with_capacity(service.operations.len());
    for operation in &service.operations {
        let name = operation.name.to_token();
        let doc = method_doc(operation, &service.security_schemes);
        let response = operation.response_enum.to_token();
        let args = emit_method_args(operation)?;
        methods.push(quote! {
            #doc
            fn #name(&self, #(#args),*) -> impl std::future::Future<Output = #response> + Send;
        });
    }
    return Ok(quote! {
        /// Server behaviour: implement one method per operation.
        pub trait Api: Clone + Send + Sync + 'static {
            #(#methods)*
        }
    });
}

/// The doc comment of a trait method: the operation's own description, then the
/// security the document names for it.
///
/// The generator emits no check for that security, because verifying a
/// credential needs application knowledge it does not have: which key, which
/// issuer, and which claim names which user. Naming the schemes is what it can
/// do, so an implementer does not have to read the document to tell a public
/// operation from a protected one.
fn method_doc(operation: &Operation, schemes: &[SecurityScheme]) -> TokenStream {
    if operation.security.is_empty() {
        return doc_attr(&operation.doc);
    }

    let mut lines: Vec<String> = Vec::new();
    if let Some(text) = &operation.doc {
        lines.push(text.clone());
        lines.push(String::new());
    }
    lines.push("# Security".to_owned());
    lines.push(String::new());
    lines.push("The document names these security schemes for this operation:".to_owned());
    lines.push(String::new());
    for key in &operation.security {
        lines.push(format!("- {}", requirement_line(key, schemes)));
    }
    lines.push(String::new());
    if operation.security.len() > 1 {
        // `security::required_keys` unions the alternatives and the
        // conjunctions, so past one key the list no longer says which it was.
        lines.push(
            "This list is the union of every alternative the document gives, so it may be a choice between schemes rather than all of them. Read `security` in the document for the exact rule."
                .to_owned(),
        );
        lines.push(String::new());
    }
    lines.push(
        "This generator emits no check. Enforce it in a layer around the router: this method receives only the parameters the operation declares, not the credential."
            .to_owned(),
    );
    return doc_lines(&lines);
}

/// One security scheme, named and located.
///
/// Where the credential sits is the part a server needs, because it has to read
/// the credential itself.
fn requirement_line(key: &str, schemes: &[SecurityScheme]) -> String {
    let Some(scheme) = schemes.iter().find(|scheme| return scheme.key == key) else {
        // The document names a scheme it never declares. The client rejects
        // that; a server has nothing to reject, so the doc says what it knows.
        return format!("`{key}`, which `components.securitySchemes` does not declare");
    };
    let location = match &scheme.kind {
        SecuritySchemeKind::HttpBearer => "a bearer token in the `Authorization` header".to_owned(),
        SecuritySchemeKind::HttpBasic => "basic credentials in the `Authorization` header".to_owned(),
        SecuritySchemeKind::ApiKeyHeader(name) => format!("an API key in the `{name}` header"),
        SecuritySchemeKind::ApiKeyQuery(name) => format!("an API key in the `{name}` query parameter"),
        SecuritySchemeKind::ApiKeyCookie(name) => format!("an API key in the `{name}` cookie"),
        // The reason held here is written for the client, which refuses to send
        // such a credential. A server reads credentials rather than sending
        // them, so the key alone is what this can honestly state.
        SecuritySchemeKind::Unsupported(_) => {
            return format!("`{key}`, a scheme this generator has no built-in support for");
        }
    };
    return format!("`{key}`: {location}");
}

/// The typed arguments (path parameters, query struct, header struct, then JSON
/// body) of an operation method.
fn emit_method_args(operation: &Operation) -> Result<Vec<TokenStream>> {
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

/// Emit an operation's response enum together with its `IntoResponse` impl.
/// Emit an operation's `IntoResponse` impl, turning the shared response enum
/// (emitted by [`crate::emit::operation`]) into an axum response.
fn emit_into_response(operation: &Operation) -> Result<TokenStream> {
    let name = operation.response_enum.to_token();
    let mut arms = Vec::with_capacity(operation.responses.len());
    for case in &operation.responses {
        let variant = case.variant.to_token();
        let arm = if case.headers.is_empty() {
            match &case.status {
                ResponseStatus::Fixed(code) => emit_fixed_response(&name, &variant, *code, &case.body)?,
                ResponseStatus::Default | ResponseStatus::Range(_) => {
                    emit_dynamic_response(&name, &variant, &case.body)?
                }
            }
        } else {
            emit_response_with_headers(&name, &variant, case)?
        };
        arms.push(arm);
    }
    return Ok(quote! {
        impl axum::response::IntoResponse for #name {
            fn into_response(self) -> axum::response::Response {
                match self {
                    #(#arms)*
                }
            }
        }
    });
}

/// Emit the `IntoResponse` arm for a fixed status code, whose value is known at
/// generation time and emitted as a compile-time constant.
fn emit_fixed_response(
    name: &proc_macro2::Ident,
    variant: &proc_macro2::Ident,
    code: u16,
    body: &Option<ResponseBody>,
) -> Result<TokenStream> {
    let code = proc_macro2::Literal::u16_unsuffixed(code);
    let status = quote! {
        const STATUS: axum::http::StatusCode = match axum::http::StatusCode::from_u16(#code) {
            Ok(status) => status,
            Err(_) => panic!("oapi-codegen emitted an invalid HTTP status code"),
        };
    };
    let arm = match body {
        Some(ResponseBody::Single(body)) => {
            let term = response_body_term(body.kind);
            quote! {
                #name::#variant(body) => {
                    #status
                    (STATUS, #term).into_response()
                }
            }
        }
        Some(ResponseBody::Negotiated(negotiated)) => {
            let arms = negotiated_response_arms(negotiated, &quote! { STATUS }, false);
            quote! {
                #name::#variant(body) => {
                    #status
                    match body {
                        #(#arms)*
                    }
                }
            }
        }
        None => {
            quote! {
                #name::#variant => {
                    #status
                    STATUS.into_response()
                }
            }
        }
    };
    return Ok(arm);
}

/// Emit the `IntoResponse` arm for a `default`/range response, whose concrete
/// status code is not fixed by the spec and is carried in the variant.
fn emit_dynamic_response(
    name: &proc_macro2::Ident,
    variant: &proc_macro2::Ident,
    body: &Option<ResponseBody>,
) -> Result<TokenStream> {
    let arm = match body {
        Some(ResponseBody::Single(body)) => {
            let term = response_body_term(body.kind);
            quote! {
                #name::#variant(status, body) => (status, #term).into_response(),
            }
        }
        Some(ResponseBody::Negotiated(negotiated)) => {
            let arms = negotiated_response_arms(negotiated, &quote! { status }, false);
            quote! {
                #name::#variant(status, body) => match body {
                    #(#arms)*
                },
            }
        }
        None => {
            quote! {
                #name::#variant(status) => status.into_response(),
            }
        }
    };
    return Ok(arm);
}

/// Emit the `Router` builder, grouping operations that share a path so they map
/// onto a single axum route with multiple method handlers.
fn emit_router(service: &Service) -> TokenStream {
    let routes = service
        .operations
        .chunk_by(|left, right| return left.path == right.path)
        .map(|group| {
            let [first, ..] = group else {
                unreachable!("chunk_by never yields an empty group");
            };
            let path = &first.path;
            let mut method_router = TokenStream::new();
            for (index, operation) in group.iter().enumerate() {
                let routing = format_ident!("{}", operation.method);
                let handler = axum_handler_name(&operation.name).to_token();
                if index == 0 {
                    method_router = quote! { axum::routing::#routing(#handler::<T>) };
                } else {
                    method_router = quote! { #method_router.#routing(#handler::<T>) };
                }
            }
            return quote! { .route(#path, #method_router) };
        });
    return quote! {
        /// Build an axum `Router` that dispatches each route to `api`.
        pub fn router<T: Api>(api: T) -> axum::Router {
            axum::Router::new()
                #(#routes)*
                .with_state(api)
        }
    };
}

/// Emit an operation's internal handler: extract the typed inputs, call the
/// `Api` method, and return its response (which is `IntoResponse`).
fn emit_handler(operation: &Operation, visibility: HandlerVisibility) -> Result<TokenStream> {
    let handler = axum_handler_name(&operation.name).to_token();
    let method = operation.name.to_token();
    let response = operation.response_enum.to_token();

    let mut extractors = vec![quote! {
        axum::extract::State(api): axum::extract::State<T>
    }];
    let mut call_args: Vec<TokenStream> = Vec::new();
    if !operation.path_params.is_empty() {
        let names: Vec<proc_macro2::Ident> = operation
            .path_params
            .iter()
            .map(|param| {
                return param.name.to_token();
            })
            .collect();
        let mut types = Vec::with_capacity(operation.path_params.len());
        for param in &operation.path_params {
            types.push(emit_type(&param.ty)?);
        }
        if let ([name], [ty]) = (names.as_slice(), types.as_slice()) {
            extractors.push(quote! { axum::extract::Path(#name): axum::extract::Path<#ty> });
        } else {
            extractors.push(quote! { axum::extract::Path((#(#names),*)): axum::extract::Path<(#(#types),*)> });
        }
        for name in &names {
            call_args.push(quote! { #name });
        }
    }
    if let Some(query) = &operation.query {
        let ty = query.name.to_token();
        extractors.push(quote! { axum_extra::extract::Query(query): axum_extra::extract::Query<#ty> });
        call_args.push(quote! { query });
    }
    if let Some(headers) = &operation.headers {
        let ty = headers.name.to_token();
        extractors.push(quote! { headers: #ty });
        call_args.push(quote! { headers });
    }
    if let Some(cookies) = &operation.cookies {
        let ty = cookies.name.to_token();
        extractors.push(quote! { cookies: #ty });
        call_args.push(quote! { cookies });
    }
    match &operation.request {
        Some(RequestPayload::Single(body)) => {
            let ty = emit_type(&body.ty)?;
            extractors.push(body_extractor(body.kind, &ty));
            call_args.push(quote! { body });
        }
        Some(RequestPayload::Multipart(multipart)) => {
            let ty = multipart.name.to_token();
            extractors.push(quote! { body: #ty });
            call_args.push(quote! { body });
        }
        Some(RequestPayload::Negotiated(request)) => {
            let ty = request.name.to_token();
            extractors.push(quote! { body: #ty });
            call_args.push(quote! { body });
        }
        None => {}
    }

    let vis = visibility.to_tokens();
    return Ok(quote! {
        #vis async fn #handler<T: Api>(#(#extractors),*) -> #response {
            api.#method(#(#call_args),*).await
        }
    });
}

/// Emit a struct-variant definition and `IntoResponse` arm for a response that
/// declares headers. Field order: `status` (dynamic responses only), `body`
/// (when present), then one field per declared header (required → `T`,
/// optional → `Option<T>`). Header values are formatted with `to_string()` and
/// inserted best-effort — a value that cannot encode as a `HeaderValue` is
/// skipped rather than panicking.
fn emit_response_with_headers(
    name: &proc_macro2::Ident,
    variant: &proc_macro2::Ident,
    case: &ResponseCase,
) -> Result<TokenStream> {
    let dynamic = !matches!(case.status, ResponseStatus::Fixed(_));

    // Destructure pattern (bind every field the shared variant defines).
    let mut binds: Vec<TokenStream> = Vec::new();
    if dynamic {
        binds.push(quote! { status });
    }
    if case.body.is_some() {
        binds.push(quote! { body });
    }
    let header_idents: Vec<proc_macro2::Ident> = case.headers.iter().map(|h| return h.name.to_token()).collect();
    for ident in &header_idents {
        binds.push(quote! { #ident });
    }

    // Status expression: constant for fixed, bound `status` for dynamic.
    let status_expr = match &case.status {
        ResponseStatus::Fixed(code) => {
            let code = proc_macro2::Literal::u16_unsuffixed(*code);
            quote! {
                {
                    const STATUS: axum::http::StatusCode = match axum::http::StatusCode::from_u16(#code) {
                        Ok(status) => status,
                        Err(_) => panic!("oapi-codegen emitted an invalid HTTP status code"),
                    };
                    STATUS
                }
            }
        }
        ResponseStatus::Default | ResponseStatus::Range(_) => quote! { status },
    };

    // Header insertions (best-effort).
    let mut inserts = Vec::with_capacity(case.headers.len());
    for header in &case.headers {
        inserts.push(emit_response_header_insert(header));
    }

    let render = match &case.body {
        Some(ResponseBody::Negotiated(negotiated)) => {
            let arms = negotiated_response_arms(negotiated, &quote! { response_status }, true);
            quote! {
                let response_status = #status_expr;
                match body {
                    #(#arms)*
                }
            }
        }
        Some(ResponseBody::Single(body)) => {
            let term = response_body_term(body.kind);
            quote! { return (#status_expr, header_map, #term).into_response(); }
        }
        None => quote! { return (#status_expr, header_map).into_response(); },
    };

    let arm = quote! {
        #name::#variant { #(#binds),* } => {
            let mut header_map = axum::http::HeaderMap::new();
            #(#inserts)*
            #render
        }
    };

    return Ok(arm);
}

/// Emit the insertion of one response header into `header_map`. A required
/// header whose value cannot encode as a `HeaderValue` aborts the arm with a
/// `500`, since dropping it will violate the declared contract. An optional
/// header is inserted only when present, and is silently skipped if its value
/// cannot encode.
fn emit_response_header_insert(header: &ResponseHeader) -> TokenStream {
    let ident = header.name.to_token();
    let lower_name = header.header_name.to_ascii_lowercase();
    if header.required {
        return quote! {
            let value = match axum::http::HeaderValue::from_str(&#ident.to_string()) {
                Ok(value) => value,
                Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
            header_map.insert(axum::http::HeaderName::from_static(#lower_name), value);
        };
    }
    return quote! {
        if let Some(#ident) = #ident {
            if let Ok(value) = axum::http::HeaderValue::from_str(&#ident.to_string()) {
                header_map.insert(axum::http::HeaderName::from_static(#lower_name), value);
            }
        }
    };
}

/// Emit the hand-written `FromRequestParts` implementation for an operation's
/// header struct (defined by [`crate::emit::operation`]).
///
/// Header values are read and parsed individually from the request parts, so
/// the struct cannot derive `serde::Deserialize` the way the query struct does.
/// A missing required header, a non-text value, or a value that fails to parse
/// yields a `400 Bad Request` carrying a short plaintext reason.
fn emit_headers_extractor(headers: &Headers) -> Result<TokenStream> {
    let name = headers.name.to_token();

    let mut bindings = Vec::with_capacity(headers.params.len());
    let mut idents = Vec::with_capacity(headers.params.len());
    for param in &headers.params {
        let ident = param.name.to_token();
        bindings.push(emit_header_binding(param)?);
        idents.push(ident);
    }

    return Ok(quote! {
        impl<S> axum::extract::FromRequestParts<S> for #name
        where
            S: Send + Sync,
        {
            type Rejection = (axum::http::StatusCode, String);

            async fn from_request_parts(
                parts: &mut axum::http::request::Parts,
                _state: &S,
            ) -> Result<Self, Self::Rejection> {
                #(#bindings)*
                return Ok(Self { #(#idents),* });
            }
        }
    });
}

/// Emit the `let <field> = ...` binding that reads and parses one header,
/// returning a `400` on a missing required header or an unparseable value.
///
/// String headers are taken verbatim. other scalars are `trim()`-ed before
/// parsing, since HTTP permits optional surrounding whitespace (OWS) that
/// `FromStr` will otherwise reject.
fn emit_header_binding(param: &HeaderParam) -> Result<TokenStream> {
    let ident = param.name.to_token();
    let header_name = &param.header_name;
    let missing_msg = format!("missing required header `{header_name}`");
    let not_text_msg = format!("header `{header_name}` is not valid text");

    let value_expr = if matches!(param.ty, RustType::String) {
        quote! { text.to_owned() }
    } else {
        let ty = emit_type(&param.ty)?;
        let invalid_msg = format!("header `{header_name}` has an invalid value");
        quote! {
            match text.trim().parse::<#ty>() {
                Ok(parsed) => parsed,
                Err(_) => return Err((axum::http::StatusCode::BAD_REQUEST, #invalid_msg.to_owned())),
            }
        }
    };

    let present = if param.required {
        value_expr
    } else {
        quote! { Some(#value_expr) }
    };

    let absent = if param.required {
        quote! { return Err((axum::http::StatusCode::BAD_REQUEST, #missing_msg.to_owned())) }
    } else {
        quote! { None }
    };

    return Ok(quote! {
        let #ident = match parts.headers.get(#header_name) {
            Some(value) => {
                let text = match value.to_str() {
                    Ok(text) => text,
                    Err(_) => return Err((axum::http::StatusCode::BAD_REQUEST, #not_text_msg.to_owned())),
                };
                #present
            }
            None => #absent,
        };
    });
}

/// Emit the hand-written `FromRequestParts` implementation for an operation's
/// cookie struct (defined by [`crate::emit::operation`]), backed by
/// `axum_extra`'s `CookieJar`. A missing required cookie or a value that fails
/// to parse yields a `400 Bad Request` with a short plaintext reason.
fn emit_cookies_extractor(cookies: &Cookies) -> Result<TokenStream> {
    let name = cookies.name.to_token();

    let mut bindings = Vec::with_capacity(cookies.params.len());
    let mut idents = Vec::with_capacity(cookies.params.len());
    for param in &cookies.params {
        let ident = param.name.to_token();
        bindings.push(emit_cookie_binding(param)?);
        idents.push(ident);
    }

    return Ok(quote! {
        impl<S> axum::extract::FromRequestParts<S> for #name
        where
            S: Send + Sync,
        {
            type Rejection = (axum::http::StatusCode, String);

            async fn from_request_parts(
                parts: &mut axum::http::request::Parts,
                _state: &S,
            ) -> Result<Self, Self::Rejection> {
                let jar = axum_extra::extract::CookieJar::from_headers(&parts.headers);
                #(#bindings)*
                return Ok(Self { #(#idents),* });
            }
        }
    });
}

/// Emit the `let <field> = ...` binding that reads and parses one cookie from
/// the jar, returning a `400` on a missing required cookie or an unparseable
/// value. String cookies are taken verbatim. other scalars are `trim()`-ed
/// before parsing.
fn emit_cookie_binding(param: &CookieParam) -> Result<TokenStream> {
    let ident = param.name.to_token();
    let cookie_name = &param.cookie_name;

    // An optional string cookie maps straight through with no fallible step, so
    // emit `.map()` rather than a `match { Some => Some, None => None }` (which
    // trips clippy's `manual_map` in the generated output).
    if !param.required && matches!(param.ty, RustType::String) {
        return Ok(quote! {
            let #ident = jar.get(#cookie_name).map(|cookie| return cookie.value().to_owned());
        });
    }

    let missing_msg = format!("missing required cookie `{cookie_name}`");

    let value_expr = if matches!(param.ty, RustType::String) {
        quote! { cookie.value().to_owned() }
    } else {
        let ty = emit_type(&param.ty)?;
        let invalid_msg = format!("cookie `{cookie_name}` has an invalid value");
        quote! {
            match cookie.value().trim().parse::<#ty>() {
                Ok(parsed) => parsed,
                Err(_) => return Err((axum::http::StatusCode::BAD_REQUEST, #invalid_msg.to_owned())),
            }
        }
    };

    let present = if param.required {
        value_expr
    } else {
        quote! { Some(#value_expr) }
    };

    let absent = if param.required {
        quote! { return Err((axum::http::StatusCode::BAD_REQUEST, #missing_msg.to_owned())) }
    } else {
        quote! { None }
    };

    return Ok(quote! {
        let #ident = match jar.get(#cookie_name) {
            Some(cookie) => #present,
            None => #absent,
        };
    });
}

/// Emit the hand-written `axum::extract::FromRequest` implementation for an
/// operation's `multipart/form-data` struct (defined by
/// [`crate::emit::operation`]).
///
/// axum has no typed multipart extractor, so the implementation drives
/// `axum::extract::Multipart`, reads each declared field (text scalars are
/// parsed with `FromStr`. binary/file fields are read as raw bytes), and
/// returns a `400 Bad Request` with a short plaintext reason on a missing
/// required field or an unparseable value. Unknown fields are ignored. a
/// repeated field keeps its last value.
fn emit_multipart_extractor(multipart: &Multipart) -> Result<TokenStream> {
    let name = multipart.name.to_token();

    let mut accumulators = Vec::with_capacity(multipart.fields.len());
    let mut arms = Vec::with_capacity(multipart.fields.len());
    let mut inits = Vec::with_capacity(multipart.fields.len());
    for field in &multipart.fields {
        let ident = field.rust_name.to_token();
        let ty = emit_type(&field.ty)?;
        accumulators.push(quote! { let mut #ident: Option<#ty> = None; });
        arms.push(emit_multipart_arm(field)?);
        inits.push(emit_multipart_init(field));
    }

    return Ok(quote! {
        impl<S> axum::extract::FromRequest<S> for #name
        where
            S: Send + Sync,
        {
            type Rejection = (axum::http::StatusCode, String);

            async fn from_request(
                request: axum::extract::Request,
                state: &S,
            ) -> Result<Self, Self::Rejection> {
                let mut multipart = <axum::extract::Multipart as axum::extract::FromRequest<S>>::from_request(
                    request,
                    state,
                )
                .await
                .map_err(|error| return (axum::http::StatusCode::BAD_REQUEST, error.to_string()))?;
                #(#accumulators)*
                while let Some(field) = multipart
                    .next_field()
                    .await
                    .map_err(|error| return (axum::http::StatusCode::BAD_REQUEST, error.to_string()))?
                {
                    let field_name = field.name().map(|name| return name.to_owned());
                    match field_name.as_deref() {
                        #(#arms)*
                        _ => {}
                    }
                }
                return Ok(Self { #(#inits),* });
            }
        }
    });
}

/// Emit the `match` arm that reads one multipart field into its accumulator: raw
/// bytes for a file field, a verbatim `String`, or a `trim()`-parsed scalar.
fn emit_multipart_arm(field: &MultipartField) -> Result<TokenStream> {
    let ident = field.rust_name.to_token();
    let wire = &field.wire_name;

    let read = if field.is_file {
        quote! {
            let value = field
                .bytes()
                .await
                .map_err(|error| return (axum::http::StatusCode::BAD_REQUEST, error.to_string()))?;
            #ident = Some(value.to_vec());
        }
    } else if matches!(field.ty, RustType::String) {
        quote! {
            let value = field
                .text()
                .await
                .map_err(|error| return (axum::http::StatusCode::BAD_REQUEST, error.to_string()))?;
            #ident = Some(value);
        }
    } else {
        let ty = emit_type(&field.ty)?;
        let invalid_msg = format!("multipart field `{wire}` has an invalid value");
        quote! {
            let text = field
                .text()
                .await
                .map_err(|error| return (axum::http::StatusCode::BAD_REQUEST, error.to_string()))?;
            let value = match text.trim().parse::<#ty>() {
                Ok(parsed) => parsed,
                Err(_) => return Err((axum::http::StatusCode::BAD_REQUEST, #invalid_msg.to_owned())),
            };
            #ident = Some(value);
        }
    };

    return Ok(quote! {
        Some(#wire) => {
            #read
        }
    });
}

/// Emit the struct-literal initialiser for one multipart field. A non-optional
/// field is unwrapped with a `400` on absence. An optional field passes its
/// `Option<..>` accumulator straight through via field-init shorthand.
fn emit_multipart_init(field: &MultipartField) -> TokenStream {
    let ident = field.rust_name.to_token();
    if field.optional {
        return quote! { #ident };
    }
    let missing_msg = format!("missing required multipart field `{}`", field.wire_name);
    return quote! {
        #ident: #ident.ok_or((axum::http::StatusCode::BAD_REQUEST, #missing_msg.to_owned()))?
    };
}

/// Emit the hand-written `axum::extract::FromRequest` implementation for an
/// operation's `Content-Type`-dispatched request body enum (defined by
/// [`crate::emit::operation`]).
///
/// The implementation reads the request's `Content-Type` header, matches it
/// against each declared content type in priority order (JSON > form > text),
/// and delegates to the matching axum extractor. A recognised type that fails
/// to decode yields a `400 Bad Request`. An unrecognised or missing type yields
/// a `415 Unsupported Media Type`.
fn emit_request_body_extractor(request: &NegotiatedBody) -> Result<TokenStream> {
    let name = request.name.to_token();

    let mut arms = Vec::with_capacity(request.variants.len());
    for variant in &request.variants {
        arms.push(emit_request_body_arm(&name, variant)?);
    }

    return Ok(quote! {
        impl<S> axum::extract::FromRequest<S> for #name
        where
            S: Send + Sync,
        {
            type Rejection = (axum::http::StatusCode, String);

            async fn from_request(
                request: axum::extract::Request,
                state: &S,
            ) -> Result<Self, Self::Rejection> {
                let content_type = request
                    .headers()
                    .get(axum::http::header::CONTENT_TYPE)
                    .and_then(|value| return value.to_str().ok())
                    .map(|value| {
                        return value.split(';').next().unwrap_or(value).trim().to_ascii_lowercase();
                    })
                    .unwrap_or_default();
                #(#arms)*
                if content_type.is_empty() {
                    return Err((
                        axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        "missing `Content-Type` header".to_owned(),
                    ));
                }
                return Err((
                    axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    format!("unsupported content type `{content_type}`"),
                ));
            }
        }
    });
}

/// Emit the `from_request` arm that decodes one content-type representation:
/// when the runtime `content_type` matches this variant's kind, delegate to the
/// matching axum extractor and wrap the value in the variant.
fn emit_request_body_arm(name: &proc_macro2::Ident, variant: &BodyVariant) -> Result<TokenStream> {
    let ident = variant.variant.to_token();
    let ty = emit_type(&variant.body.ty)?;
    let test = request_content_type_test(variant.body.kind);
    let decode = match variant.body.kind {
        BodyKind::Json => quote! {
            let axum::Json(body) = <axum::Json<#ty> as axum::extract::FromRequest<S>>::from_request(request, state)
                .await
                .map_err(|error| return (axum::http::StatusCode::BAD_REQUEST, error.to_string()))?;
        },
        BodyKind::Form => quote! {
            let axum::Form(body) = <axum::Form<#ty> as axum::extract::FromRequest<S>>::from_request(request, state)
                .await
                .map_err(|error| return (axum::http::StatusCode::BAD_REQUEST, error.to_string()))?;
        },
        BodyKind::Text => quote! {
            let body = <String as axum::extract::FromRequest<S>>::from_request(request, state)
                .await
                .map_err(|error| return (axum::http::StatusCode::BAD_REQUEST, error.to_string()))?;
        },
        BodyKind::Multipart => {
            unreachable!("multipart never participates in content-type negotiation")
        }
    };
    return Ok(quote! {
        if #test {
            #decode
            return Ok(#name::#ident(body));
        }
    });
}

/// The boolean test matching a normalised (lowercased, parameter-stripped)
/// `content_type` against a negotiated request variant's kind. Mirrors the
/// generator's `media_type_kind` classification.
fn request_content_type_test(kind: BodyKind) -> TokenStream {
    return match kind {
        BodyKind::Json => quote! { content_type == "application/json" || content_type.ends_with("+json") },
        BodyKind::Form => quote! { content_type == "application/x-www-form-urlencoded" },
        BodyKind::Text => quote! { content_type == "text/plain" },
        BodyKind::Multipart => {
            unreachable!("multipart never participates in content-type negotiation")
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naming::Case;
    use crate::naming::to_ident;

    fn scheme(key: &str, kind: SecuritySchemeKind) -> SecurityScheme {
        return SecurityScheme {
            key: key.to_owned(),
            field: to_ident(key, Case::Snake),
            kind,
            doc: None,
        };
    }

    #[test]
    fn every_scheme_kind_says_where_the_credential_sits() {
        let schemes = vec![
            scheme("bearerAuth", SecuritySchemeKind::HttpBearer),
            scheme("basicAuth", SecuritySchemeKind::HttpBasic),
            scheme("headerKey", SecuritySchemeKind::ApiKeyHeader("X-API-Key".to_owned())),
            scheme("queryKey", SecuritySchemeKind::ApiKeyQuery("api_key".to_owned())),
            scheme("cookieKey", SecuritySchemeKind::ApiKeyCookie("SESSION".to_owned())),
        ];
        let cases = [
            (
                "bearerAuth",
                "`bearerAuth`: a bearer token in the `Authorization` header",
            ),
            (
                "basicAuth",
                "`basicAuth`: basic credentials in the `Authorization` header",
            ),
            ("headerKey", "`headerKey`: an API key in the `X-API-Key` header"),
            ("queryKey", "`queryKey`: an API key in the `api_key` query parameter"),
            ("cookieKey", "`cookieKey`: an API key in the `SESSION` cookie"),
        ];
        for (key, expected) in cases {
            assert_eq!(requirement_line(key, &schemes), expected);
        }
    }

    /// The client rejects a scheme the document never declares. A server has
    /// nothing to reject, so the note has to stand on the key alone.
    #[test]
    fn an_undeclared_scheme_is_still_named() {
        let line = requirement_line("ghost", &[]);
        assert_eq!(line, "`ghost`, which `components.securitySchemes` does not declare");
    }

    #[test]
    fn an_unsupported_scheme_drops_the_client_side_reason() {
        let schemes = vec![scheme(
            "oauth2",
            SecuritySchemeKind::Unsupported("the client cannot send this".to_owned()),
        )];
        let line = requirement_line("oauth2", &schemes);
        assert_eq!(line, "`oauth2`, a scheme this generator has no built-in support for");
    }
}
