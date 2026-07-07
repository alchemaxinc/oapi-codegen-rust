//! Emitting the axum server interface as token streams.

use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;

use crate::emit::doc_attr;
use crate::emit::emit_type;
use crate::emit::models::emit_struct;
use crate::error::Result;
use crate::ir::Body;
use crate::ir::CookieParam;
use crate::ir::Cookies;
use crate::ir::HeaderParam;
use crate::ir::Headers;
use crate::ir::Operation;
use crate::ir::ResponseCase;
use crate::ir::ResponseHeader;
use crate::ir::ResponseStatus;
use crate::ir::RustType;
use crate::ir::Service;
use crate::naming::operations::axum_handler_name;

/// The axum server-interface emitter.
pub struct AxumServer;

impl crate::emit::ServerEmitter for AxumServer {
    fn emit(&self, service: &Service) -> Result<Vec<TokenStream>> {
        return service_items(service);
    }
}

/// The handler extractor pattern + type for a request body of the given kind.
fn body_extractor(kind: crate::ir::BodyKind, ty: &TokenStream) -> TokenStream {
    return match kind {
        crate::ir::BodyKind::Json => quote! { axum::Json(body): axum::Json<#ty> },
        crate::ir::BodyKind::Text => quote! { body: String },
        crate::ir::BodyKind::Form => quote! { axum::Form(body): axum::Form<#ty> },
    };
}

/// The response tuple term that renders a body of the given kind.
fn response_body_term(kind: crate::ir::BodyKind) -> TokenStream {
    return match kind {
        crate::ir::BodyKind::Json => quote! { axum::Json(body) },
        crate::ir::BodyKind::Text => quote! { body },
        crate::ir::BodyKind::Form => quote! { axum::Form(body) },
    };
}

/// Emit the axum server interface: the `Api` trait, per-operation response
/// enums, the `Router` builder, and the internal handler functions.
fn service_items(service: &Service) -> Result<Vec<TokenStream>> {
    let mut items = Vec::new();
    for operation in &service.operations {
        if let Some(query) = &operation.query {
            items.push(emit_struct(query)?);
        }
        if let Some(headers) = &operation.headers {
            items.extend(emit_headers(headers)?);
        }
        if let Some(cookies) = &operation.cookies {
            items.extend(emit_cookies(cookies)?);
        }
    }
    items.push(emit_trait(service)?);
    for operation in &service.operations {
        let (enum_def, into_response) = emit_response_enum(operation)?;
        items.push(enum_def);
        items.push(into_response);
    }
    items.push(emit_router(service));
    for operation in &service.operations {
        items.push(emit_handler(operation)?);
    }
    return Ok(items);
}

/// Emit the `Api` trait, one async method per operation.
fn emit_trait(service: &Service) -> Result<TokenStream> {
    let mut methods = Vec::with_capacity(service.operations.len());
    for operation in &service.operations {
        let name = operation.name.to_token();
        let doc = doc_attr(&operation.doc);
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
    if let Some(body) = &operation.body {
        let ty = emit_type(&body.ty)?;
        args.push(quote! { body: #ty });
    }
    return Ok(args);
}

/// Emit an operation's response enum together with its `IntoResponse` impl.
fn emit_response_enum(operation: &Operation) -> Result<(TokenStream, TokenStream)> {
    let name = operation.response_enum.to_token();
    let mut variants = Vec::with_capacity(operation.responses.len());
    let mut arms = Vec::with_capacity(operation.responses.len());
    for case in &operation.responses {
        let variant = case.variant.to_token();
        let doc = doc_attr(&case.doc);
        let (variant_def, arm) = if case.headers.is_empty() {
            match &case.status {
                ResponseStatus::Fixed(code) => emit_fixed_response(&name, &variant, *code, &case.body)?,
                ResponseStatus::Default | ResponseStatus::Range(_) => {
                    emit_dynamic_response(&name, &variant, &case.body)?
                }
            }
        } else {
            emit_response_with_headers(&name, &variant, case)?
        };
        variants.push(quote! { #doc #variant_def });
        arms.push(arm);
    }
    let doc = doc_attr(&operation.doc);
    let enum_def = quote! {
        #doc
        pub enum #name {
            #(#variants),*
        }
    };
    let into_response = quote! {
        impl axum::response::IntoResponse for #name {
            fn into_response(self) -> axum::response::Response {
                match self {
                    #(#arms)*
                }
            }
        }
    };
    return Ok((enum_def, into_response));
}

/// Emit the variant and `IntoResponse` arm for a fixed status code, whose value
/// is known at generation time and emitted as a compile-time constant.
fn emit_fixed_response(
    name: &proc_macro2::Ident,
    variant: &proc_macro2::Ident,
    code: u16,
    body: &Option<Body>,
) -> Result<(TokenStream, TokenStream)> {
    let code = proc_macro2::Literal::u16_unsuffixed(code);
    let status = quote! {
        const STATUS: axum::http::StatusCode = match axum::http::StatusCode::from_u16(#code) {
            Ok(status) => status,
            Err(_) => panic!("oapi-codegen emitted an invalid HTTP status code"),
        };
    };
    let result = match body {
        Some(body) => {
            let ty = emit_type(&body.ty)?;
            let variant_def = quote! { #variant(#ty) };
            let term = response_body_term(body.kind);
            let arm = quote! {
                #name::#variant(body) => {
                    #status
                    (STATUS, #term).into_response()
                }
            };
            (variant_def, arm)
        }
        None => {
            let variant_def = quote! { #variant };
            let arm = quote! {
                #name::#variant => {
                    #status
                    STATUS.into_response()
                }
            };
            (variant_def, arm)
        }
    };
    return Ok(result);
}

/// Emit the variant and `IntoResponse` arm for a `default`/range response, whose
/// concrete status code is not fixed by the spec and is therefore carried in the
/// variant and supplied by the handler at runtime.
fn emit_dynamic_response(
    name: &proc_macro2::Ident,
    variant: &proc_macro2::Ident,
    body: &Option<Body>,
) -> Result<(TokenStream, TokenStream)> {
    let result = match body {
        Some(body) => {
            let ty = emit_type(&body.ty)?;
            let variant_def = quote! { #variant(axum::http::StatusCode, #ty) };
            let term = response_body_term(body.kind);
            let arm = quote! {
                #name::#variant(status, body) => (status, #term).into_response(),
            };
            (variant_def, arm)
        }
        None => {
            let variant_def = quote! { #variant(axum::http::StatusCode) };
            let arm = quote! {
                #name::#variant(status) => status.into_response(),
            };
            (variant_def, arm)
        }
    };
    return Ok(result);
}

/// Emit the `Router` builder, grouping operations that share a path so they map
/// onto a single axum route with multiple method handlers.
fn emit_router(service: &Service) -> TokenStream {
    let mut routes = Vec::new();
    let mut index = 0;
    while index < service.operations.len() {
        let path = &service.operations[index].path;
        let mut method_router = TokenStream::new();
        let mut first = true;
        while index < service.operations.len() && service.operations[index].path == *path {
            let operation = &service.operations[index];
            let routing = format_ident!("{}", operation.method);
            let handler = axum_handler_name(&operation.name).to_token();
            if first {
                method_router = quote! { axum::routing::#routing(#handler::<T>) };
                first = false;
            } else {
                method_router = quote! { #method_router.#routing(#handler::<T>) };
            }
            index += 1;
        }
        routes.push(quote! { .route(#path, #method_router) });
    }
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
fn emit_handler(operation: &Operation) -> Result<TokenStream> {
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
        if operation.path_params.len() == 1 {
            let name = &names[0];
            let ty = &types[0];
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
    if let Some(body) = &operation.body {
        let ty = emit_type(&body.ty)?;
        extractors.push(body_extractor(body.kind, &ty));
        call_args.push(quote! { body });
    }

    return Ok(quote! {
        async fn #handler<T: Api>(#(#extractors),*) -> #response {
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
) -> Result<(TokenStream, TokenStream)> {
    let dynamic = !matches!(case.status, ResponseStatus::Fixed(_));

    // Field definitions.
    let mut field_defs: Vec<TokenStream> = Vec::new();
    if dynamic {
        field_defs.push(quote! { status: axum::http::StatusCode });
    }
    if let Some(body) = &case.body {
        let ty = emit_type(&body.ty)?;
        field_defs.push(quote! { body: #ty });
    }
    let mut header_field_defs = Vec::with_capacity(case.headers.len());
    for header in &case.headers {
        header_field_defs.push(emit_response_header_field(header)?);
    }

    let variant_def = quote! {
        #variant {
            #(#field_defs,)*
            #(#header_field_defs)*
        }
    };

    // Destructure pattern (bind every field we defined).
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

    let body_term = case
        .body
        .as_ref()
        .map(|b| {
            let t = response_body_term(b.kind);
            return quote! { , #t };
        })
        .unwrap_or_default();

    let arm = quote! {
        #name::#variant { #(#binds),* } => {
            let mut header_map = axum::http::HeaderMap::new();
            #(#inserts)*
            return (#status_expr, header_map #body_term).into_response();
        }
    };

    return Ok((variant_def, arm));
}

/// Emit one struct-variant field for a response header (required → `T`,
/// optional → `Option<T>`), with its doc attribute.
fn emit_response_header_field(header: &ResponseHeader) -> Result<TokenStream> {
    let ident = header.name.to_token();
    let doc = doc_attr(&header.doc);
    let mut ty = emit_type(&header.ty)?;
    if !header.required {
        ty = quote! { Option<#ty> };
    }
    return Ok(quote! {
        #doc
        #ident: #ty,
    });
}

/// Emit the best-effort insertion of one response header into `header_map`.
/// Uses a lowercased static header name; a value that fails to encode as a
/// `HeaderValue` is skipped (never panics).
fn emit_response_header_insert(header: &ResponseHeader) -> TokenStream {
    let ident = header.name.to_token();
    let lower_name = header.header_name.to_ascii_lowercase();
    let insert = quote! {
        if let Ok(value) = axum::http::HeaderValue::from_str(&#ident.to_string()) {
            header_map.insert(axum::http::HeaderName::from_static(#lower_name), value);
        }
    };
    if header.required {
        return insert;
    }
    return quote! {
        if let Some(#ident) = #ident {
            #insert
        }
    };
}

/// Emit a header struct and its hand-written `FromRequestParts` implementation.
///
/// Header values are read and parsed individually from the request parts, so
/// the struct cannot derive `serde::Deserialize` the way the query struct does.
/// A missing required header, a non-text value, or a value that fails to parse
/// yields a `400 Bad Request` carrying a short plaintext reason.
fn emit_headers(headers: &Headers) -> Result<Vec<TokenStream>> {
    let name = headers.name.to_token();

    let mut field_defs = Vec::with_capacity(headers.params.len());
    let mut bindings = Vec::with_capacity(headers.params.len());
    let mut idents = Vec::with_capacity(headers.params.len());
    for param in &headers.params {
        let ident = param.name.to_token();
        let doc = doc_attr(&param.doc);
        let mut ty = emit_type(&param.ty)?;
        if !param.required {
            ty = quote! { Option<#ty> };
        }
        field_defs.push(quote! {
            #doc
            pub #ident: #ty,
        });
        bindings.push(emit_header_binding(param)?);
        idents.push(ident);
    }

    let struct_def = quote! {
        #[derive(Debug, Clone)]
        pub struct #name {
            #(#field_defs)*
        }
    };

    let extractor = quote! {
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
    };

    return Ok(vec![struct_def, extractor]);
}

/// Emit the `let <field> = ...;` binding that reads and parses one header,
/// returning a `400` on a missing required header or an unparseable value.
///
/// String headers are taken verbatim; other scalars are `trim()`-ed before
/// parsing, since HTTP permits optional surrounding whitespace (OWS) that
/// `FromStr` would otherwise reject.
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

/// Emit a cookie struct and its hand-written `FromRequestParts` implementation,
/// backed by `axum_extra`'s `CookieJar`. A missing required cookie or a value
/// that fails to parse yields a `400 Bad Request` with a short plaintext reason.
fn emit_cookies(cookies: &Cookies) -> Result<Vec<TokenStream>> {
    let name = cookies.name.to_token();

    let mut field_defs = Vec::with_capacity(cookies.params.len());
    let mut bindings = Vec::with_capacity(cookies.params.len());
    let mut idents = Vec::with_capacity(cookies.params.len());
    for param in &cookies.params {
        let ident = param.name.to_token();
        let doc = doc_attr(&param.doc);
        let mut ty = emit_type(&param.ty)?;
        if !param.required {
            ty = quote! { Option<#ty> };
        }
        field_defs.push(quote! {
            #doc
            pub #ident: #ty,
        });
        bindings.push(emit_cookie_binding(param)?);
        idents.push(ident);
    }

    let struct_def = quote! {
        #[derive(Debug, Clone)]
        pub struct #name {
            #(#field_defs)*
        }
    };

    let extractor = quote! {
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
    };

    return Ok(vec![struct_def, extractor]);
}

/// Emit the `let <field> = ...;` binding that reads and parses one cookie from
/// the jar, returning a `400` on a missing required cookie or an unparseable
/// value. String cookies are taken verbatim; other scalars are `trim()`-ed
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
