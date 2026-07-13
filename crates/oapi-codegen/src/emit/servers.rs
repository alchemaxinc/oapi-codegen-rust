//! Emitting the [`ServerUrls`] IR as top-level Rust items.
//!
//! Variable-free servers become `pub const` string constants; servers with
//! `{placeholder}`s become builder functions returning `Result<String, String>`
//! that substitute each variable and reject any leftover placeholder. Enum
//! variables get a dedicated enum type exposing an `as_str` accessor.
//!
//! Building the output as token streams (rather than string templates) makes
//! spec-controlled text — descriptions, URLs, enum values — inert: it can only
//! land inside a string literal or doc attribute, never as executable source.

use proc_macro2::TokenStream;
use quote::quote;

use crate::emit::doc_attr;
use crate::error::Result;
use crate::ir::ServerUrl;
use crate::ir::ServerUrlBuilder;
use crate::ir::ServerUrlConst;
use crate::ir::ServerUrlEnum;
use crate::ir::ServerUrlParamType;
use crate::ir::ServerUrls;

/// Emit every server-URL item as its own top-level token stream, enum types
/// first so the builders that reference them are already in scope.
pub fn emit_server_urls(server_urls: &ServerUrls) -> Result<Vec<TokenStream>> {
    let mut items = Vec::new();
    for enom in &server_urls.enums {
        items.extend(emit_enum(enom));
    }
    for server in &server_urls.servers {
        items.push(emit_server(server));
    }
    return Ok(items);
}

/// Emit an enum type, its `as_str` accessor, and (when a default is declared)
/// its `Default` impl, as separate top-level items.
fn emit_enum(enom: &ServerUrlEnum) -> Vec<TokenStream> {
    let name = enom.name.to_token();
    let doc = doc_attr(&enom.doc);
    let variants = enom.variants.iter().map(|variant| {
        let ident = variant.name.to_token();
        return quote! { #ident, };
    });
    let arms = enom.variants.iter().map(|variant| {
        let ident = variant.name.to_token();
        let value = &variant.value;
        return quote! { #name::#ident => #value, };
    });
    let type_item = quote! {
        #doc
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum #name {
            #(#variants)*
        }
    };
    let impl_item = quote! {
        impl #name {
            /// The wire value substituted into the server URL.
            pub fn as_str(&self) -> &'static str {
                return match self {
                    #(#arms)*
                };
            }
        }
    };
    let mut items = vec![type_item, impl_item];
    if let Some(default) = &enom.default {
        let default = default.to_token();
        items.push(quote! {
            impl Default for #name {
                fn default() -> Self {
                    return #name::#default;
                }
            }
        });
    }
    return items;
}

/// Emit a single server as either a constant or a builder function.
fn emit_server(server: &ServerUrl) -> TokenStream {
    return match server {
        ServerUrl::Const(konst) => emit_const(konst),
        ServerUrl::Builder(builder) => emit_builder(builder),
    };
}

/// Emit a variable-free server URL as a string constant.
fn emit_const(konst: &ServerUrlConst) -> TokenStream {
    let name = konst.name.to_token();
    let doc = doc_attr(&konst.doc);
    let url = &konst.url;
    return quote! {
        #doc
        pub const #name: &str = #url;
    };
}

/// Emit a server URL with placeholders as a builder that substitutes each
/// variable and rejects any that remain unresolved.
fn emit_builder(builder: &ServerUrlBuilder) -> TokenStream {
    let name = builder.name.to_token();
    let doc = doc_attr(&builder.doc);
    let template = &builder.url_template;
    let params = builder.params.iter().map(|param| {
        let ident = param.ident.to_token();
        return match &param.ty {
            ServerUrlParamType::Str => quote! { #ident: &str },
            ServerUrlParamType::Enum(enum_name) => {
                let ty = enum_name.to_token();
                quote! { #ident: #ty }
            }
        };
    });
    let replacements = builder.params.iter().map(|param| {
        let ident = param.ident.to_token();
        let token = format!("{{{}}}", param.placeholder);
        return match &param.ty {
            ServerUrlParamType::Str => quote! { url = url.replace(#token, #ident); },
            ServerUrlParamType::Enum(_) => quote! { url = url.replace(#token, #ident.as_str()); },
        };
    });
    return quote! {
        #doc
        pub fn #name(#(#params),*) -> Result<String, String> {
            let mut url = String::from(#template);
            #(#replacements)*
            if url.contains('{') || url.contains('}') {
                return Err(format!("server URL still contains an unresolved placeholder: {url}"));
            }
            return Ok(url);
        }
    };
}
