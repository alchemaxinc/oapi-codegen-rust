//! Emitting model items (structs, enums, aliases) as token streams.

use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;

use crate::emit::doc_attr;
use crate::emit::emit_type;
use crate::error::Result;
use crate::ir::Alias;
use crate::ir::DefaultValue;
use crate::ir::Deprecation;
use crate::ir::Enum;
use crate::ir::EnumKind;
use crate::ir::Field;
use crate::ir::ForeignDerives;
use crate::ir::Item;
use crate::ir::RustType;
use crate::ir::StringVariant;
use crate::ir::Struct;
use crate::ir::UnionVariant;
use crate::naming::RustIdent;

/// The full derive set for one generated model: its serde traits, plus which of
/// `Debug`, `Clone`, `PartialEq` the model can carry.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ModelDerives {
    /// Which serde traits the model's API direction needs.
    pub serde: SerdeDerives,
    /// Which of the three non-serde traits every foreign type the model reaches
    /// implements. See [`ForeignDerives`].
    pub foreign: ForeignDerives,
}

impl ModelDerives {
    /// Both serde traits and all three non-serde traits — the derive set the
    /// generator emitted for every model before either narrowing existed. Used
    /// when a model's usage direction is unknown, as in models-only generation.
    pub(crate) fn both() -> Self {
        return Self {
            serde: SerdeDerives::both(),
            foreign: ForeignDerives::default(),
        };
    }
}

/// Which serde traits a generated model derives.
///
/// A type is only ever serialized in the direction its API position uses: a
/// server serializes response bodies and deserializes request bodies, a client
/// does the reverse. Deriving a serde trait the type never needs will impose an
/// unsatisfiable bound on a reused `x-rust-type` target (for example forcing
/// `Deserialize` on a response-only type that a project only serializes), so the
/// derive set is narrowed to the directions the type is actually used in. When a
/// type is used in both directions — or both a server and a client are generated
/// — both traits are derived, matching the previous unconditional behaviour.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SerdeDerives {
    /// Derive `serde::Serialize`.
    pub serialize: bool,
    /// Derive `serde::Deserialize`.
    pub deserialize: bool,
}

impl SerdeDerives {
    /// Both serde traits — the safe default when a type's usage direction is
    /// unknown (models-only generation) or it is used in both directions.
    pub(crate) fn both() -> Self {
        return Self {
            serialize: true,
            deserialize: true,
        };
    }
}

/// Render a single top-level item with the given derive set.
pub(crate) fn emit_item(item: &Item, derives: ModelDerives) -> Result<TokenStream> {
    let tokens = match item {
        Item::Struct(strukt) => emit_struct(strukt, derives)?,
        Item::Enum(enom) => emit_enum(enom, derives)?,
        // An alias derives nothing. `type X = Y;` carries no derive, so a foreign
        // type reached through one constrains the models that name the alias and
        // not the alias itself.
        Item::Alias(alias) => emit_alias(alias)?,
    };
    return Ok(tokens);
}

/// The derive list applied to a generated type: the requested serde traits
/// followed by each of `Debug`, `Clone`, `PartialEq` that every foreign type the
/// model reaches implements.
///
/// A derive the model cannot satisfy is dropped rather than emitted and left to
/// fail, because the failure lands on generated code the consumer must not edit.
/// Dropping it costs the consumer a trait on that one model, and they can still
/// write the impl by hand.
fn derive_attr(derives: ModelDerives) -> TokenStream {
    let mut parts: Vec<TokenStream> = Vec::new();
    if derives.serde.serialize {
        parts.push(quote! { serde::Serialize });
    }
    if derives.serde.deserialize {
        parts.push(quote! { serde::Deserialize });
    }
    if derives.foreign.debug {
        parts.push(quote! { Debug });
    }
    if derives.foreign.clone {
        parts.push(quote! { Clone });
    }
    if derives.foreign.partial_eq {
        parts.push(quote! { PartialEq });
    }
    // An empty `#[derive()]` is valid Rust, but it is noise in output a human
    // reads, so emit nothing at all when every trait was dropped.
    if parts.is_empty() {
        return quote! {};
    }
    return quote! {
        #[derive(#(#parts),*)]
    };
}

/// The derive attribute for a generated type that carries no serde trait: the
/// intersection of what the emitter wants and what the type's contents allow.
///
/// Both masks are a [`ForeignDerives`], which reads oddly for the request side but
/// is exactly the right shape. The two are the same three flags, and one is
/// intersected with the other. A header struct wants `Debug` and `Clone` and never
/// `PartialEq`, so it passes that as the request.
pub(crate) fn plain_derive_attr(requested: ForeignDerives, allowed: ForeignDerives) -> TokenStream {
    return derive_attr(ModelDerives {
        serde: SerdeDerives {
            serialize: false,
            deserialize: false,
        },
        foreign: requested.intersect(allowed),
    });
}

/// The request mask for a type deriving `Debug` and `Clone` only, such as an
/// operation's header or cookie input struct. Those are read field by field and
/// never compared.
pub(crate) const DEBUG_AND_CLONE: ForeignDerives = ForeignDerives {
    debug: true,
    clone: true,
    partial_eq: false,
};

/// The request mask for a type deriving all three, such as a response enum.
pub(crate) const DEBUG_CLONE_AND_EQ: ForeignDerives = ForeignDerives {
    debug: true,
    clone: true,
    partial_eq: true,
};

/// Render a `#[deprecated]` / `#[deprecated(note = "...")]` attribute, if any.
fn deprecated_attr(deprecated: &Option<Deprecation>) -> TokenStream {
    return match deprecated {
        None => quote! {},
        Some(Deprecation { note: None }) => quote! { #[deprecated] },
        Some(Deprecation { note: Some(note) }) => quote! { #[deprecated(note = #note)] },
    };
}

/// Render a `struct` item with the given serde derive set.
pub(crate) fn emit_struct(strukt: &Struct, derives: ModelDerives) -> Result<TokenStream> {
    let name = strukt.name.to_token();
    let doc = doc_attr(&strukt.doc);
    let deprecated = deprecated_attr(&strukt.deprecated);
    let serde = derives.serde;
    let derive_attr = derive_attr(derives);
    let has_serde = serde.serialize || serde.deserialize;

    let mut fields = Vec::with_capacity(strukt.fields.len());
    let mut defaults = Vec::new();
    for field in &strukt.fields {
        fields.push(emit_field(field, has_serde, &strukt.name)?);
        if let Some(value) = &field.default {
            defaults.push(emit_default_fn(field, value)?);
        }
    }
    // serde needs a path to call. An associated function keeps these out of the
    // crate root, where every generated type lives. Field names are unique
    // within a struct, so the names built from them are too.
    let defaults = if defaults.is_empty() {
        quote! {}
    } else {
        quote! { impl #name { #(#defaults)* } }
    };

    let additional = match &strukt.additional_properties {
        Some(element) => {
            let ty = emit_type(element)?;
            // The `flatten` attribute is only meaningful when the struct derives
            // a serde trait. Without one it will be an orphaned `#[serde(..)]`.
            let flatten = if has_serde {
                quote! { #[serde(flatten)] }
            } else {
                quote! {}
            };
            quote! {
                #flatten
                pub additional_properties: std::collections::HashMap<String, #ty>,
            }
        }
        None => quote! {},
    };

    // `additionalProperties: false` becomes `deny_unknown_fields`. The attribute
    // is read by the `Deserialize` derive only, so it is gated on that derive and
    // not on `has_serde`. A serialize-only type reads no unknown key, so it has
    // none to deny, and the attribute on it would be orphaned.
    let deny_unknown = if strukt.deny_unknown_fields && serde.deserialize {
        quote! { #[serde(deny_unknown_fields)] }
    } else {
        quote! {}
    };

    return Ok(quote! {
        #doc
        #derive_attr
        #deny_unknown
        #deprecated
        pub struct #name {
            #(#fields)*
            #additional
        }

        #defaults
    });
}

/// The name that serde calls to fill an absent property.
fn default_fn_name(field: &Field) -> proc_macro2::Ident {
    return format_ident!("default_{}", field.name.logical());
}

/// Render the associated function behind `#[serde(default = "..")]`.
fn emit_default_fn(field: &Field, value: &DefaultValue) -> Result<TokenStream> {
    let name = default_fn_name(field);
    let ty = emit_type(&field.ty)?;
    let expr = emit_default_value(value, &field.ty)?;
    let doc = format!(" The `default` the document gives `{}`.", field.name.logical());
    return Ok(quote! {
        #[doc = #doc]
        fn #name() -> #ty {
            #expr
        }
    });
}

/// Render a default as an expression of the field type.
fn emit_default_value(value: &DefaultValue, ty: &RustType) -> Result<TokenStream> {
    match ty {
        RustType::Option(inner) => {
            let inner = emit_default_value(value, inner)?;
            return Ok(quote! { Some(#inner) });
        }
        RustType::Boxed(inner) => {
            let inner = emit_default_value(value, inner)?;
            return Ok(quote! { Box::new(#inner) });
        }
        _ => {}
    }
    let expr = match value {
        DefaultValue::Str(text) => quote! { #text.to_owned() },
        // No suffix, so one arm serves both `i32` and `i64`. A whole number
        // still reads as a float where the field is one.
        DefaultValue::Int(number) => {
            let literal = proc_macro2::Literal::i64_unsuffixed(*number);
            quote! { #literal }
        }
        DefaultValue::Float(number) => {
            let literal = proc_macro2::Literal::f64_unsuffixed(*number);
            quote! { #literal }
        }
        DefaultValue::Bool(flag) => quote! { #flag },
        DefaultValue::Variant(variant) => {
            let owner = emit_type(ty)?;
            let variant = variant.to_token();
            quote! { #owner::#variant }
        }
        // The return type pins this to the right empty collection.
        DefaultValue::Empty => quote! { Default::default() },
    };
    return Ok(expr);
}

/// Render a single struct field. When the struct derives no serde trait,
/// `#[serde(..)]` attributes are suppressed — without a serde derive macro in
/// scope they are orphaned and fail to compile.
fn emit_field(field: &Field, has_serde: bool, owner: &RustIdent) -> Result<TokenStream> {
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
        if field.default.is_some() {
            // `to_token` keeps any `r#` prefix. A path without it does not
            // compile.
            let path = format!("{}::{}", owner.to_token(), default_fn_name(field));
            metas.push(quote! { default = #path });
        }
    }
    let serde_attr = if !has_serde || metas.is_empty() {
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

/// Render an `enum` item (string enum or untagged union) with the given serde
/// derive set.
pub(crate) fn emit_enum(enom: &Enum, derives: ModelDerives) -> Result<TokenStream> {
    let name = enom.name.to_token();
    let doc = doc_attr(&enom.doc);
    let deprecated = deprecated_attr(&enom.deprecated);
    let derive_attr = derive_attr(derives);

    let tokens = match &enom.kind {
        EnumKind::Strings(variants) => {
            let rendered = variants.iter().map(emit_string_variant);
            let rendered: Vec<TokenStream> = rendered.collect();
            quote! {
                #doc
                #derive_attr
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
                #derive_attr
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
