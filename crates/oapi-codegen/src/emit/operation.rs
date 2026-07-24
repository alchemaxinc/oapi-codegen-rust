//! Emitting the per-operation types that the server and client both use.
//!
//! An operation's query/header/cookie inputs, multipart and negotiated request
//! bodies, negotiated response bodies, and response enum are the same types
//! whichever generator consumes them, so they are emitted once here. The axum
//! server ([`crate::emit::axum`]) then adds its extractor and `IntoResponse`
//! impls for these types, and the `reqwest` client ([`crate::emit::reqwest`])
//! its request-building and decoding methods.

use proc_macro2::TokenStream;
use quote::quote;

use crate::emit::Targets;
use crate::emit::doc_attr;
use crate::emit::emit_multipart_struct;
use crate::emit::emit_negotiated_body_enum;
use crate::emit::emit_type;
use crate::emit::models::SerdeDerives;
use crate::emit::models::emit_struct;
use crate::error::Result;
use crate::ir::Cookies;
use crate::ir::Headers;
use crate::ir::Operation;
use crate::ir::RequestPayload;
use crate::ir::ResponseBody;
use crate::ir::ResponseCase;
use crate::ir::ResponseStatus;

/// Emit every type an operation contributes to the crate root: its query,
/// header, and cookie input structs, any multipart or negotiated request body,
/// the negotiated response bodies, and the response enum.
pub fn emit_operation_types(operation: &Operation, targets: Targets) -> Result<Vec<TokenStream>> {
    let mut items = Vec::new();
    if let Some(query) = &operation.query {
        // The query struct is deserialized by the server's `Query` extractor;
        // the client builds the query string field-by-field via `to_string`
        // rather than serializing the struct, so it never needs `Serialize`.
        let serde = SerdeDerives {
            serialize: false,
            deserialize: targets.server,
        };
        items.push(emit_struct(query, serde)?);
    }
    if let Some(headers) = &operation.headers {
        items.push(emit_headers_struct(headers)?);
    }
    if let Some(cookies) = &operation.cookies {
        items.push(emit_cookies_struct(cookies)?);
    }
    match &operation.request {
        Some(RequestPayload::Multipart(multipart)) => items.push(emit_multipart_struct(multipart)?),
        Some(RequestPayload::Negotiated(request)) => items.push(emit_negotiated_body_enum(request)?),
        Some(RequestPayload::Single(_)) | None => {}
    }
    for case in &operation.responses {
        if let Some(ResponseBody::Negotiated(body)) = &case.body {
            items.push(emit_negotiated_body_enum(body)?);
        }
    }
    items.push(emit_response_enum(operation)?);
    return Ok(items);
}

/// Emit the plain input struct for an operation's header parameters. Header
/// values are read and set field by field, so the struct carries only `Debug`
/// and `Clone` rather than serde derives.
fn emit_headers_struct(headers: &Headers) -> Result<TokenStream> {
    let name = headers.name.to_token();
    let mut fields = Vec::with_capacity(headers.params.len());
    for param in &headers.params {
        let field = param.name.to_token();
        let doc = doc_attr(&param.doc);
        let mut ty = emit_type(&param.ty)?;
        if !param.required {
            ty = quote! { Option<#ty> };
        }
        fields.push(quote! { #doc pub #field: #ty });
    }
    return Ok(quote! {
        #[derive(Debug, Clone)]
        pub struct #name {
            #(#fields),*
        }
    });
}

/// Emit the plain input struct for an operation's cookie parameters, mirroring
/// [`emit_headers_struct`].
fn emit_cookies_struct(cookies: &Cookies) -> Result<TokenStream> {
    let name = cookies.name.to_token();
    let mut fields = Vec::with_capacity(cookies.params.len());
    for param in &cookies.params {
        let field = param.name.to_token();
        let doc = doc_attr(&param.doc);
        let mut ty = emit_type(&param.ty)?;
        if !param.required {
            ty = quote! { Option<#ty> };
        }
        fields.push(quote! { #doc pub #field: #ty });
    }
    return Ok(quote! {
        #[derive(Debug, Clone)]
        pub struct #name {
            #(#fields),*
        }
    });
}

/// Emit an operation's response enum: one variant per declared response.
///
/// A `default`/range response carries the concrete [`http::StatusCode`] the
/// server chose. A declared response header is carried as `T` when the spec
/// marks it required and `Option<T>` when optional; the client decoder treats a
/// required header as mandatory, erroring when it is missing or unparsable.
fn emit_response_enum(operation: &Operation) -> Result<TokenStream> {
    let name = operation.response_enum.to_token();
    let doc = doc_attr(&operation.doc);
    let mut variants = Vec::with_capacity(operation.responses.len());
    for case in &operation.responses {
        let case_doc = doc_attr(&case.doc);
        let variant_def = response_variant_def(case)?;
        variants.push(quote! { #case_doc #variant_def });
    }
    return Ok(quote! {
        #doc
        #[derive(Debug, Clone, PartialEq)]
        pub enum #name {
            #(#variants),*
        }
    });
}

/// Emit the variant definition for one response case: a tuple variant carrying
/// the status (dynamic responses only) and body, or a struct variant when the
/// response also declares headers.
fn response_variant_def(case: &ResponseCase) -> Result<TokenStream> {
    let variant = case.variant.to_token();
    let dynamic = !matches!(case.status, ResponseStatus::Fixed(_));
    let body_ty = response_body_type(&case.body)?;

    if case.headers.is_empty() {
        let variant_def = match (&body_ty, dynamic) {
            (None, false) => quote! { #variant },
            (None, true) => quote! { #variant(http::StatusCode) },
            (Some(ty), false) => quote! { #variant(#ty) },
            (Some(ty), true) => quote! { #variant(http::StatusCode, #ty) },
        };
        return Ok(variant_def);
    }

    let mut field_defs = Vec::new();
    if dynamic {
        field_defs.push(quote! { status: http::StatusCode });
    }
    if let Some(ty) = &body_ty {
        field_defs.push(quote! { body: #ty });
    }
    for header in &case.headers {
        let field = header.name.to_token();
        let ty = emit_type(&header.ty)?;
        let doc = doc_attr(&header.doc);
        let field_ty = if header.required {
            quote! { #ty }
        } else {
            quote! { Option<#ty> }
        };
        field_defs.push(quote! { #doc #field: #field_ty });
    }
    return Ok(quote! { #variant { #(#field_defs),* } });
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
