//! Emitting model items (structs, enums, aliases) as token streams.

use proc_macro2::TokenStream;
use quote::quote;

use crate::emit::doc_attr;
use crate::emit::emit_type;
use crate::error::Result;
use crate::ir::Alias;
use crate::ir::Deprecation;
use crate::ir::Enum;
use crate::ir::EnumKind;
use crate::ir::Field;
use crate::ir::Item;
use crate::ir::StringVariant;
use crate::ir::Struct;
use crate::ir::UnionVariant;

/// Render a single top-level item.
pub(crate) fn emit_item(item: &Item) -> Result<TokenStream> {
    let tokens = match item {
        Item::Struct(strukt) => emit_struct(strukt)?,
        Item::Enum(enom) => emit_enum(enom)?,
        Item::Alias(alias) => emit_alias(alias)?,
    };
    return Ok(tokens);
}

/// The derive list applied to every generated type.
fn derives() -> TokenStream {
    return quote! {
        #[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
    };
}

/// Render a `#[deprecated]` / `#[deprecated(note = "...")]` attribute, if any.
fn deprecated_attr(deprecated: &Option<Deprecation>) -> TokenStream {
    return match deprecated {
        None => quote! {},
        Some(Deprecation { note: None }) => quote! { #[deprecated] },
        Some(Deprecation { note: Some(note) }) => quote! { #[deprecated(note = #note)] },
    };
}

/// Render a `struct` item.
pub(crate) fn emit_struct(strukt: &Struct) -> Result<TokenStream> {
    let name = strukt.name.to_token();
    let doc = doc_attr(&strukt.doc);
    let deprecated = deprecated_attr(&strukt.deprecated);
    let derives = derives();

    let mut fields = Vec::with_capacity(strukt.fields.len());
    for field in &strukt.fields {
        fields.push(emit_field(field)?);
    }

    let additional = match &strukt.additional_properties {
        Some(element) => {
            let ty = emit_type(element)?;
            quote! {
                #[serde(flatten)]
                pub additional_properties: std::collections::HashMap<String, #ty>,
            }
        }
        None => quote! {},
    };

    return Ok(quote! {
        #doc
        #derives
        #deprecated
        pub struct #name {
            #(#fields)*
            #additional
        }
    });
}

/// Render a single struct field.
fn emit_field(field: &Field) -> Result<TokenStream> {
    let name = field.name.to_token();
    let ty = emit_type(&field.ty)?;
    let doc = doc_attr(&field.doc);
    let deprecated = deprecated_attr(&field.deprecated);

    let mut metas: Vec<TokenStream> = Vec::new();
    if field.serde_skip {
        metas.push(quote! { skip });
    } else {
        if let Some(rename) = &field.rename {
            metas.push(quote! { rename = #rename });
        }
        let omit_empty = field.omit_empty.unwrap_or(!field.required);
        if omit_empty && field.ty.is_option() {
            metas.push(quote! { skip_serializing_if = "Option::is_none" });
        }
    }
    let serde_attr = if metas.is_empty() {
        quote! {}
    } else {
        quote! { #[serde(#(#metas),*)] }
    };

    return Ok(quote! {
        #doc
        #serde_attr
        #deprecated
        pub #name: #ty,
    });
}

/// Render an `enum` item (string enum or untagged union).
fn emit_enum(enom: &Enum) -> Result<TokenStream> {
    let name = enom.name.to_token();
    let doc = doc_attr(&enom.doc);
    let deprecated = deprecated_attr(&enom.deprecated);
    let derives = derives();

    let tokens = match &enom.kind {
        EnumKind::Strings(variants) => {
            let rendered = variants.iter().map(emit_string_variant);
            let rendered: Vec<TokenStream> = rendered.collect();
            quote! {
                #doc
                #derives
                #deprecated
                pub enum #name {
                    #(#rendered)*
                }
            }
        }
        EnumKind::Union(variants) => {
            let mut rendered = Vec::with_capacity(variants.len());
            for variant in variants {
                rendered.push(emit_union_variant(variant)?);
            }
            quote! {
                #doc
                #derives
                #[serde(untagged)]
                #deprecated
                pub enum #name {
                    #(#rendered)*
                }
            }
        }
    };
    return Ok(tokens);
}

/// Render one unit variant of a string enum.
fn emit_string_variant(variant: &StringVariant) -> TokenStream {
    let name = variant.name.to_token();
    let doc = doc_attr(&variant.doc);
    let serde_attr = match &variant.rename {
        Some(rename) => quote! { #[serde(rename = #rename)] },
        None => quote! {},
    };
    return quote! {
        #doc
        #serde_attr
        #name,
    };
}

/// Render one newtype variant of a union enum.
fn emit_union_variant(variant: &UnionVariant) -> Result<TokenStream> {
    let name = variant.name.to_token();
    let ty = emit_type(&variant.ty)?;
    return Ok(quote! {
        #name(#ty),
    });
}

/// Render a `type X = Y;` alias.
fn emit_alias(alias: &Alias) -> Result<TokenStream> {
    let name = alias.name.to_token();
    let ty = emit_type(&alias.ty)?;
    let doc = doc_attr(&alias.doc);
    let deprecated = deprecated_attr(&alias.deprecated);
    return Ok(quote! {
        #doc
        #deprecated
        pub type #name = #ty;
    });
}
