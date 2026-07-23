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

    for name in service_refs(service) {
        if reachable.insert(name.clone()) {
            worklist.push(name);
        }
    }

    let index: HashMap<&str, &Item> = module.items.iter().map(|item| return (item.name(), item)).collect();

    while let Some(name) = worklist.pop() {
        let Some(item) = index.get(name.as_str()) else {
            continue;
        };
        for referenced in item_refs(item) {
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

/// The named type a `ty` references, if any (recursing through containers).
///
/// A `RustType` names at most one component model: containers (`Vec`, `Map`,
/// `Option`) wrap a single inner type, and every other variant is either a
/// leaf `Named` or carries no model reference.
fn named_ref(ty: &RustType) -> Option<String> {
    return match ty {
        RustType::Named(name) => Some(canonical(name)),
        RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) => named_ref(inner),
        _ => None,
    };
}

/// The named types a module item refers to (its fields/variants/alias).
fn item_refs(item: &Item) -> Vec<String> {
    return match item {
        Item::Struct(s) => struct_refs(s),
        Item::Enum(enumeration) => match &enumeration.kind {
            EnumKind::Strings(_) => Vec::new(),
            EnumKind::Union(variants) => variants
                .iter()
                .filter_map(|variant| return named_ref(&variant.ty))
                .collect(),
        },
        Item::Alias(alias) => named_ref(&alias.ty).into_iter().collect(),
    };
}

/// The named types a struct's fields and `additionalProperties` refer to.
fn struct_refs(s: &Struct) -> Vec<String> {
    return s
        .fields
        .iter()
        .filter_map(|field| return named_ref(&field.ty))
        .chain(s.additional_properties.as_ref().and_then(named_ref))
        .collect();
}

/// Every named type the service's operations reference directly.
fn service_refs(service: &Service) -> Vec<String> {
    let mut refs = Vec::new();
    for operation in &service.operations {
        for param in &operation.path_params {
            refs.extend(named_ref(&param.ty));
        }
        if let Some(query) = &operation.query {
            refs.extend(struct_refs(query));
        }
        if let Some(headers) = &operation.headers {
            for param in &headers.params {
                refs.extend(named_ref(&param.ty));
            }
        }
        if let Some(cookies) = &operation.cookies {
            for param in &cookies.params {
                refs.extend(named_ref(&param.ty));
            }
        }
        if let Some(request) = &operation.request {
            match request {
                RequestPayload::Single(body) => refs.extend(named_ref(&body.ty)),
                RequestPayload::Multipart(multipart) => {
                    for field in &multipart.fields {
                        refs.extend(named_ref(&field.ty));
                    }
                }
                RequestPayload::Negotiated(negotiated) => {
                    for variant in &negotiated.variants {
                        refs.extend(named_ref(&variant.body.ty));
                    }
                }
            }
        }
        for response in &operation.responses {
            match &response.body {
                Some(ResponseBody::Single(body)) => refs.extend(named_ref(&body.ty)),
                Some(ResponseBody::Negotiated(negotiated)) => {
                    for variant in &negotiated.variants {
                        refs.extend(named_ref(&variant.body.ty));
                    }
                }
                None => {}
            }
            for header in &response.headers {
                refs.extend(named_ref(&header.ty));
            }
        }
    }
    return refs;
}
