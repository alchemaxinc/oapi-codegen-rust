//! Pruning component-schema models that no generated operation uses.
//!
//! This mirrors `oapi-codegen`'s default behaviour of pruning unused
//! `#/components` entries before generation: only the schema models reachable
//! from an operation survive. Roots are the types an operation references
//! directly (path/query/header/cookie parameters, request and response bodies,
//! and response headers); reachability then follows each retained model's own
//! type references to a fixpoint, so a chain `A -> B -> C` rooted at an
//! operation keeps all three while a model referenced by nothing is dropped.
//!
//! Import-mapped (`External`) types are emitted into other modules and never
//! appear as items here, so they are naturally excluded from the reachable set.
//! Pruning is only applied when a server or client is generated (operations
//! provide the roots); models-only generation keeps every schema.

use std::collections::BTreeSet;
use std::collections::HashMap;

use crate::ir::EnumKind;
use crate::ir::Item;
use crate::ir::Module;
use crate::ir::RequestPayload;
use crate::ir::ResponseBody;
use crate::ir::RustType;
use crate::ir::Service;
use crate::ir::Struct;
use crate::naming::Case;
use crate::naming::to_ident;

/// Drop every module item not reachable from `service`'s operations.
pub fn prune_unused_models(module: &mut Module, service: &Service) {
    let mut reachable: BTreeSet<String> = BTreeSet::new();
    let mut worklist: Vec<String> = Vec::new();

    let mut roots = Vec::new();
    collect_service_refs(service, &mut roots);
    for name in roots {
        if reachable.insert(name.clone()) {
            worklist.push(name);
        }
    }

    let index: HashMap<&str, &Item> = module.items.iter().map(|item| return (item.name(), item)).collect();

    while let Some(name) = worklist.pop() {
        let Some(item) = index.get(name.as_str()) else {
            continue;
        };
        let mut refs = Vec::new();
        collect_item_refs(item, &mut refs);
        for referenced in refs {
            if reachable.insert(referenced.clone()) {
                worklist.push(referenced);
            }
        }
    }

    module.items.retain(|item| return reachable.contains(item.name()));
}

/// The canonical reachability key for a named type: its `PascalCase` identifier,
/// matching how [`Item::name`] and the emitter derive a type's Rust name.
fn canonical(name: &str) -> String {
    return to_ident(name, Case::Pascal).logical().to_owned();
}

/// Record every named-type reference within `ty` (recursing through containers).
fn collect_named(ty: &RustType, out: &mut Vec<String>) {
    match ty {
        RustType::Named(name) => out.push(canonical(name)),
        RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) => {
            collect_named(inner, out);
        }
        _ => {}
    }
}

/// Record the named types a module item refers to (its fields/variants/alias).
fn collect_item_refs(item: &Item, out: &mut Vec<String>) {
    match item {
        Item::Struct(strukt) => collect_struct_refs(strukt, out),
        Item::Enum(enumeration) => match &enumeration.kind {
            EnumKind::Strings(_) => {}
            EnumKind::Union(variants) => {
                for variant in variants {
                    collect_named(&variant.ty, out);
                }
            }
        },
        Item::Alias(alias) => collect_named(&alias.ty, out),
    }
}

/// Record the named types a struct's fields and `additionalProperties` refer to.
fn collect_struct_refs(strukt: &Struct, out: &mut Vec<String>) {
    for field in &strukt.fields {
        collect_named(&field.ty, out);
    }
    if let Some(additional) = &strukt.additional_properties {
        collect_named(additional, out);
    }
}

/// Whether the service directly references any named component model.
///
/// When both the server and client are emitted into submodules, each submodule
/// needs `use super::*;` to bring the root-level models into scope — but only if
/// it actually references one, since an unused glob import fails `-D warnings`.
pub fn references_models(service: &Service) -> bool {
    let mut refs = Vec::new();
    collect_service_refs(service, &mut refs);
    return !refs.is_empty();
}

/// Record every named type the service's operations reference directly.
fn collect_service_refs(service: &Service, out: &mut Vec<String>) {
    for operation in &service.operations {
        for param in &operation.path_params {
            collect_named(&param.ty, out);
        }
        if let Some(query) = &operation.query {
            collect_struct_refs(query, out);
        }
        if let Some(headers) = &operation.headers {
            for param in &headers.params {
                collect_named(&param.ty, out);
            }
        }
        if let Some(cookies) = &operation.cookies {
            for param in &cookies.params {
                collect_named(&param.ty, out);
            }
        }
        if let Some(request) = &operation.request {
            match request {
                RequestPayload::Single(body) => collect_named(&body.ty, out),
                RequestPayload::Multipart(multipart) => {
                    for field in &multipart.fields {
                        collect_named(&field.ty, out);
                    }
                }
                RequestPayload::Negotiated(negotiated) => {
                    for variant in &negotiated.variants {
                        collect_named(&variant.body.ty, out);
                    }
                }
            }
        }
        for response in &operation.responses {
            match &response.body {
                Some(ResponseBody::Single(body)) => collect_named(&body.ty, out),
                Some(ResponseBody::Negotiated(negotiated)) => {
                    for variant in &negotiated.variants {
                        collect_named(&variant.body.ty, out);
                    }
                }
                None => {}
            }
            for header in &response.headers {
                collect_named(&header.ty, out);
            }
        }
    }
}
