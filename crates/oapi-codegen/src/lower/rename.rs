//! Resolving top-level type names across the lowered IR.
//!
//! A generated type name can diverge from the naive `to_ident(schema_name)` for
//! two reasons: an explicit `x-rust-name` override, or collision de-confliction
//! when distinct schema names collapse onto the same Rust identifier. Because
//! every reference to a schema lowers to a [`RustType::Named`] holding the
//! original schema name, resolving the type also requires rewriting those
//! references so they point at the final identifier. The item names themselves
//! are set from the same resolution map during lowering (see
//! [`crate::lower::schema`]); this pass only rewrites the `Named` references
//! left pointing at the original name.

use std::collections::HashMap;
use std::collections::HashSet;

use openapiv3::ReferenceOr;

use crate::ir::EnumKind;
use crate::ir::Item;
use crate::ir::Module;
use crate::ir::RequestPayload;
use crate::ir::ResponseBody;
use crate::ir::RustType;
use crate::ir::Service;
use crate::loader::Spec;
use crate::naming::Case;
use crate::naming::deconflict_ident;
use crate::naming::to_ident;

/// The `x-rust-name` extension: override a generated type or field identifier.
const X_RUST_NAME: &str = "x-rust-name";

/// Map of original schema name to the final Rust type identifier that references
/// to it must resolve to, for every top-level schema whose emitted name differs
/// from the naive `to_ident(name)`. Two sources contribute an entry:
///
/// * an `x-rust-name` override (inline schemas only), and
/// * collision de-confliction, when distinct schema names collapse onto the same
///   Rust identifier (e.g. `foo-bar` and `fooBar` both becoming `FooBar`) — the
///   later schema in document order gains a numeric suffix (`FooBar2`).
///
/// Schemas whose emitted name is unchanged are omitted, so the common case
/// yields an empty map and reference rewriting is skipped entirely.
pub fn type_renames(spec: &Spec) -> HashMap<String, String> {
    let mut resolved = HashMap::new();
    let mut seen = HashSet::new();
    for (name, entry) in spec.schemas() {
        let effective = match entry {
            ReferenceOr::Item(schema) => schema
                .schema_data
                .extensions
                .get(X_RUST_NAME)
                .and_then(|value| return value.as_str())
                .unwrap_or(name),
            ReferenceOr::Reference { .. } => name.as_str(),
        };
        let ident = deconflict_ident(to_ident(effective, Case::Pascal), &mut seen);
        if ident.logical() != to_ident(name, Case::Pascal).logical() {
            resolved.insert(name.clone(), ident.logical().to_owned());
        }
    }
    return resolved;
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
    visit_service_types(service, &mut |ty| {
        if let RustType::Named(name) = ty
            && let Some(custom) = renames.get(name.as_str())
        {
            *name = custom.clone();
        }
    });
}

/// Qualify every model reference in `service` with the `super::` path.
///
/// When the server and client generators are emitted into sibling submodules
/// while the shared models stay at the crate root, a bare `Named` reference in a
/// submodule would resolve against that submodule's own items first — an
/// operation's response enum, for instance, shares the model's identifier and
/// would shadow it, producing a recursive (self-referential) type. Rewriting
/// each model reference to an [`RustType::External`] rooted at `super` names the
/// crate-root model unambiguously and removes any need for a `use super::*;`
/// glob. Must run after [`crate::lower::prune_unused_models`], whose reachability
/// walk only follows `Named` references.
pub fn qualify_service_models(service: &mut Service) {
    visit_service_types(service, &mut |ty| {
        if let RustType::Named(name) = ty {
            let name = std::mem::take(name);
            *ty = RustType::External {
                module: "super".to_owned(),
                name,
            };
        }
    });
}

/// Apply `visit` to every leaf [`RustType`] referenced by the service's
/// operation signatures (path/query/header/cookie params, request and response
/// bodies, and response headers). Container types (`Vec`/`Map`/`Option`) are
/// traversed to their leaf; `visit` receives the leaf in place.
fn visit_service_types(service: &mut Service, visit: &mut dyn FnMut(&mut RustType)) {
    for operation in &mut service.operations {
        for param in &mut operation.path_params {
            visit_type(&mut param.ty, visit);
        }
        if let Some(query) = &mut operation.query {
            for field in &mut query.fields {
                visit_type(&mut field.ty, visit);
            }
            if let Some(additional) = &mut query.additional_properties {
                visit_type(additional, visit);
            }
        }
        if let Some(headers) = &mut operation.headers {
            for param in &mut headers.params {
                visit_type(&mut param.ty, visit);
            }
        }
        if let Some(cookies) = &mut operation.cookies {
            for param in &mut cookies.params {
                visit_type(&mut param.ty, visit);
            }
        }
        if let Some(request) = &mut operation.request {
            match request {
                RequestPayload::Single(body) => visit_type(&mut body.ty, visit),
                RequestPayload::Multipart(multipart) => {
                    for field in &mut multipart.fields {
                        visit_type(&mut field.ty, visit);
                    }
                }
                RequestPayload::Negotiated(negotiated) => {
                    for variant in &mut negotiated.variants {
                        visit_type(&mut variant.body.ty, visit);
                    }
                }
            }
        }
        for response in &mut operation.responses {
            match &mut response.body {
                Some(ResponseBody::Single(body)) => visit_type(&mut body.ty, visit),
                Some(ResponseBody::Negotiated(negotiated)) => {
                    for variant in &mut negotiated.variants {
                        visit_type(&mut variant.body.ty, visit);
                    }
                }
                None => {}
            }
            for header in &mut response.headers {
                visit_type(&mut header.ty, visit);
            }
        }
    }
}

/// Recurse container types to their leaf, applying `visit` to the leaf in place.
fn visit_type(ty: &mut RustType, visit: &mut dyn FnMut(&mut RustType)) {
    match ty {
        RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) => {
            visit_type(inner, visit);
        }
        leaf => visit(leaf),
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
