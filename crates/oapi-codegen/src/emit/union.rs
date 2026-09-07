use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;

use super::models::ModelDerives;
use super::models::SerdeDerives;
use super::models::deprecated_attr;
use super::models::derive_attr;
use crate::emit::doc_attr;
use crate::emit::emit_type;
use crate::error::Result;
use crate::ir::Enum;
use crate::ir::UnionVariant;
use crate::naming::Case;
use crate::naming::to_ident;

pub(super) fn emit_one_of(enom: &Enum, variants: &[UnionVariant], derives: ModelDerives) -> Result<TokenStream> {
    let name = enom.name.to_token();
    let doc = doc_attr(&enom.doc);
    let deprecated = deprecated_attr(&enom.deprecated);
    let derive = derive_attr(ModelDerives {
        serde: SerdeDerives {
            deserialize: false,
            ..derives.serde
        },
        ..derives
    });
    let mut payloads = Vec::new();
    let mut attempts = Vec::new();
    for variant in variants {
        let variant_name = variant.name.to_token();
        let ty = emit_type(&variant.ty)?;
        payloads.push(quote! { #variant_name(#ty), });
        attempts.push(quote! {
            if let ::std::result::Result::Ok(payload) = <#ty as serde::Deserialize>::deserialize(&value) {
                if selected.is_some() {
                    return ::std::result::Result::Err(serde::de::Error::custom(
                        "oneOf matched multiple Rust alternatives"
                    ));
                }
                selected = ::std::option::Option::Some(Self::#variant_name(payload));
            }
        });
    }
    let untagged = derives.serde.serialize.then(|| return quote! { #[serde(untagged)] });
    let deserialize = derives.serde.deserialize.then(|| {
        return quote! {
            impl<'de> serde::Deserialize<'de> for #name {
                fn deserialize<__Deserializer: serde::Deserializer<'de>>(deserializer: __Deserializer) -> ::std::result::Result<Self, __Deserializer::Error> {
                    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
                    let mut selected = ::std::option::Option::None;
                    #(#attempts)*
                    return selected.ok_or_else(|| serde::de::Error::custom(
                        "oneOf matched no Rust alternative"
                    ));
                }
            }
        };
    });
    return Ok(quote! {
        #doc
        #derive
        #untagged
        #deprecated
        pub enum #name {
            #(#payloads)*
        }

        #deserialize
    });
}

pub(super) fn emit_any_of(enom: &Enum, variants: &[UnionVariant], derives: ModelDerives) -> Result<TokenStream> {
    let name = enom.name.to_token();
    let doc = doc_attr(&enom.doc);
    let deprecated = deprecated_attr(&enom.deprecated);
    let derive = derive_attr(ModelDerives {
        serde: SerdeDerives {
            deserialize: false,
            ..derives.serde
        },
        ..derives
    });
    let transparent = derives.serde.serialize.then(|| return quote! { #[serde(transparent)] });
    let mut accessors = Vec::new();
    let mut checks = Vec::new();
    if derives.serde.deserialize {
        for variant in variants {
            let ty = emit_type(&variant.ty)?;
            let accessor = format_ident!("as_{}", to_ident(variant.name.logical(), Case::Snake).logical());
            let accessor_doc = format!("Decode the `{}` Rust alternative.", variant.name.logical());
            checks.push(quote! { <#ty as serde::Deserialize>::deserialize(&value).is_ok() });
            accessors.push(quote! {
                #[doc = #accessor_doc]
                pub fn #accessor(&self) -> ::std::result::Result<#ty, serde_json::Error> {
                    return <#ty as serde::Deserialize>::deserialize(&self.value);
                }
            });
        }
    }
    let construction = if derives.serde.deserialize {
        quote! {
            impl ::std::convert::TryFrom<serde_json::Value> for #name {
                type Error = serde_json::Error;

                fn try_from(value: serde_json::Value) -> ::std::result::Result<Self, Self::Error> {
                    if !(#(#checks)||*) {
                        return ::std::result::Result::Err(<serde_json::Error as serde::de::Error>::custom(
                            "anyOf matched no Rust alternative"
                        ));
                    }
                    return ::std::result::Result::Ok(Self { value });
                }
            }

            impl<'de> serde::Deserialize<'de> for #name {
                fn deserialize<__Deserializer: serde::Deserializer<'de>>(deserializer: __Deserializer) -> ::std::result::Result<Self, __Deserializer::Error> {
                    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
                    return <Self as ::std::convert::TryFrom<serde_json::Value>>::try_from(value).map_err(serde::de::Error::custom);
                }
            }
        }
    } else {
        quote! {
            /// Construct a raw JSON wrapper without alternative checks.
            impl ::std::convert::From<serde_json::Value> for #name {
                fn from(value: serde_json::Value) -> Self {
                    return Self { value };
                }
            }
        }
    };
    return Ok(quote! {
        #doc
        #derive
        #transparent
        #deprecated
        pub struct #name {
            value: serde_json::Value,
        }

        impl #name {
            /// Borrow the complete JSON value.
            pub fn as_value(&self) -> &serde_json::Value {
                return &self.value;
            }

            /// Consume the wrapper and return the complete JSON value.
            pub fn into_value(self) -> serde_json::Value {
                return self.value;
            }

            #(#accessors)*
        }

        #construction
    });
}
