//! Applying `x-rust-name` type renames across the lowered IR.
//!
//! A top-level schema may override its generated type name with `x-rust-name`.
//! Because every reference to that schema lowers to a [`RustType::Named`] holding
//! the original schema name, renaming the type also requires rewriting those
//! references so they resolve to the new identifier. The item names themselves
//! are set to the override during lowering (see [`crate::lower::schema`]); this
//! pass only rewrites the `Named` references left pointing at the old name.

use std::collections::HashMap;

use openapiv3::ReferenceOr;

use crate::ir::EnumKind;
use crate::ir::Item;
use crate::ir::Module;
use crate::ir::RequestPayload;
use crate::ir::ResponseBody;
use crate::ir::RustType;
use crate::ir::Service;
use crate::loader::Spec;

/// The `x-rust-name` extension: override a generated type or field identifier.
const X_RUST_NAME: &str = "x-rust-name";

/// Map of original schema name to its `x-rust-name` override, for every
/// top-level schema that declares one. Only inline schemas can carry the
/// extension, so `$ref` aliases are skipped.
pub fn type_renames(spec: &Spec) -> HashMap<String, String> {
    let mut renames = HashMap::new();
    for (name, entry) in spec.schemas() {
        let ReferenceOr::Item(schema) = entry else {
            continue;
        };
        if let Some(custom) = schema
            .schema_data
            .extensions
            .get(X_RUST_NAME)
            .and_then(|value| return value.as_str())
        {
            renames.insert(name.clone(), custom.to_owned());
        }
    }
    return renames;
}

/// Rewrite every `Named` reference in `module`'s items to honour `renames`.
pub fn rewrite_module(module: &mut Module, renames: &HashMap<String, String>) {
    if renames.is_empty() {
        return;
    }
    for item in &mut module.items {
        rewrite_item(item, renames);
    }
}

/// Rewrite every `Named` reference in `service`'s operations to honour `renames`.
pub fn rewrite_service(service: &mut Service, renames: &HashMap<String, String>) {
    if renames.is_empty() {
        return;
    }
    for operation in &mut service.operations {
        for param in &mut operation.path_params {
            rewrite_type(&mut param.ty, renames);
        }
        if let Some(query) = &mut operation.query {
            for field in &mut query.fields {
                rewrite_type(&mut field.ty, renames);
            }
            if let Some(additional) = &mut query.additional_properties {
                rewrite_type(additional, renames);
            }
        }
        if let Some(headers) = &mut operation.headers {
            for param in &mut headers.params {
                rewrite_type(&mut param.ty, renames);
            }
        }
        if let Some(cookies) = &mut operation.cookies {
            for param in &mut cookies.params {
                rewrite_type(&mut param.ty, renames);
            }
        }
        if let Some(request) = &mut operation.request {
            match request {
                RequestPayload::Single(body) => rewrite_type(&mut body.ty, renames),
                RequestPayload::Multipart(multipart) => {
                    for field in &mut multipart.fields {
                        rewrite_type(&mut field.ty, renames);
                    }
                }
                RequestPayload::Negotiated(negotiated) => {
                    for variant in &mut negotiated.variants {
                        rewrite_type(&mut variant.body.ty, renames);
                    }
                }
            }
        }
        for response in &mut operation.responses {
            match &mut response.body {
                Some(ResponseBody::Single(body)) => rewrite_type(&mut body.ty, renames),
                Some(ResponseBody::Negotiated(negotiated)) => {
                    for variant in &mut negotiated.variants {
                        rewrite_type(&mut variant.body.ty, renames);
                    }
                }
                None => {}
            }
            for header in &mut response.headers {
                rewrite_type(&mut header.ty, renames);
            }
        }
    }
}

/// Rewrite the `Named` references reachable from a single module item.
fn rewrite_item(item: &mut Item, renames: &HashMap<String, String>) {
    match item {
        Item::Struct(strukt) => {
            for field in &mut strukt.fields {
                rewrite_type(&mut field.ty, renames);
            }
            if let Some(additional) = &mut strukt.additional_properties {
                rewrite_type(additional, renames);
            }
        }
        Item::Enum(enumeration) => {
            if let EnumKind::Union(variants) = &mut enumeration.kind {
                for variant in variants {
                    rewrite_type(&mut variant.ty, renames);
                }
            }
        }
        Item::Alias(alias) => rewrite_type(&mut alias.ty, renames),
    }
}

/// Replace a `Named(old)` with `Named(new)` (recursing through containers).
fn rewrite_type(ty: &mut RustType, renames: &HashMap<String, String>) {
    match ty {
        RustType::Named(name) => {
            if let Some(custom) = renames.get(name.as_str()) {
                *name = custom.clone();
            }
        }
        RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) => {
            rewrite_type(inner, renames);
        }
        _ => {}
    }
}
