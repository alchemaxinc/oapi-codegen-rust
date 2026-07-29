//! Resolving top-level type names across the lowered IR.
//!
//! A generated type name can diverge from the naive `to_ident(schema_name)` for
//! two reasons: an explicit `x-rust-name` override, or collision de-confliction
//! when distinct schema names collapse onto the same Rust identifier. Because
//! every reference to a schema lowers to a [`RustType::Named`] holding the
//! original schema name, resolving the type also requires rewriting those
//! references so they point at the final identifier. The item names themselves
//! are set from the same resolution map during lowering (see
//! [`crate::lower::schema`]). this pass only rewrites the `Named` references
//! left pointing at the original name.

use std::collections::HashMap;
use std::collections::HashSet;

use openapiv3::ReferenceOr;

use crate::config::DEFAULT_RESPONSE_SUFFIX;
use crate::config::OUTPUT_OPTIONS_KEY;
use crate::config::RESPONSE_TYPE_SUFFIX_KEY;
use crate::emit::ReservedTypeName;
use crate::error::Result;
use crate::ir::EnumKind;
use crate::ir::Item;
use crate::ir::Module;
use crate::ir::RequestPayload;
use crate::ir::ResponseBody;
use crate::ir::RustType;
use crate::ir::Service;
use crate::loader::Spec;
use crate::naming::Case;
use crate::naming::X_RUST_NAME;
use crate::naming::deconflict_ident;
use crate::naming::to_ident;

/// Map of original schema name to the final Rust type identifier that references
/// to it must resolve to, for every top-level schema whose emitted name differs
/// from the naive `to_ident(name)`. Two sources contribute an entry:
///
/// * an `x-rust-name` override (inline schemas only), and
/// * collision de-confliction, when distinct schema names collapse onto the same
///   Rust identifier (for example `foo-bar` and `fooBar` both becoming `FooBar`) — the
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

/// Apply `visit` to every leaf [`RustType`] referenced by the service's
/// operation signatures (path/query/header/cookie params, request and response
/// bodies, and response headers). Container types (`Vec`/`Map`/`Option`) are
/// traversed to their leaf. `visit` receives the leaf in place.
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
/// Fail generation if a per-operation type name will collide with a
/// component-model name emitted in the same file.
///
/// In the flat layout, component models, per-operation types (response enums,
/// parameter structs, request/response body enums), and the requested generator
/// interfaces (`reserved`, for example the `Api` trait or `Client` struct) all share
/// the crate root. A model whose name matches one of those — most commonly a
/// schema named `<Op>Response`, or a schema literally named `Api`/`Client` —
/// will produce two items with the same name. Rather than silently rename,
/// generation fails so the author resolves the clash deliberately: rename the
/// schema with `x-rust-name`, or, for a response-enum clash, set
/// `output-options.response-type-suffix`. Only locally emitted models are
/// considered. import-mapped models are referenced through a qualified path and
/// cannot collide with a crate-root type.
pub fn check_type_name_collisions(service: &Service, module: &Module, reserved: &[ReservedTypeName]) -> Result<()> {
    let models: HashSet<&str> = module.items.iter().map(|item| return item.name()).collect();
    for name in reserved {
        if models.contains(name.name) {
            return Err(crate::error::Error::TypeNameCollision {
                name: name.name.to_owned(),
                artifact: name.description.to_owned(),
                hint: format!("rename the schema with `{X_RUST_NAME}`"),
            });
        }
    }
    for operation in &service.operations {
        ensure_free(&operation.response_enum, "response enum", true, &models)?;
        if let Some(query) = &operation.query {
            ensure_free(&query.name, "query-parameter struct", false, &models)?;
        }
        if let Some(headers) = &operation.headers {
            ensure_free(&headers.name, "header-parameter struct", false, &models)?;
        }
        if let Some(cookies) = &operation.cookies {
            ensure_free(&cookies.name, "cookie-parameter struct", false, &models)?;
        }
        match &operation.request {
            Some(RequestPayload::Multipart(multipart)) => {
                ensure_free(&multipart.name, "multipart request struct", false, &models)?;
            }
            Some(RequestPayload::Negotiated(request)) => {
                ensure_free(&request.name, "request-body enum", false, &models)?;
            }
            Some(RequestPayload::Single(_)) | None => {}
        }
        for response in &operation.responses {
            if let Some(ResponseBody::Negotiated(body)) = &response.body {
                ensure_free(&body.name, "response-body enum", false, &models)?;
            }
        }
    }
    return Ok(());
}

/// Return a [`crate::error::Error::TypeNameCollision`] when `name` is already
/// taken by an emitted component model. `is_response` selects the remedy hint:
/// for a response-enum clash it leads with the surgical, per-schema `x-rust-name`
/// fix and offers the broad `response-type-suffix` as an alternative, since that
/// suffix renames *every* response enum, not the colliding one.
fn ensure_free(
    name: &crate::naming::RustIdent,
    artifact: &str,
    is_response: bool,
    models: &HashSet<&str>,
) -> Result<()> {
    if !models.contains(name.logical()) {
        return Ok(());
    }
    let hint = if is_response {
        let default_suffix = to_ident(DEFAULT_RESPONSE_SUFFIX, Case::Pascal);
        format!(
            "give the colliding schema a different Rust name with `{X_RUST_NAME}` — a surgical, \
             per-schema fix that leaves the other response enums untouched — or, to rename every \
             response enum, set `{OUTPUT_OPTIONS_KEY}.{RESPONSE_TYPE_SUFFIX_KEY}` to a suffix other \
             than the default `{}` (for example `{RESPONSE_TYPE_SUFFIX_KEY}: Resp`, which renames \
             the enum to `<Op>Resp`)",
            default_suffix.logical(),
        )
    } else {
        format!("rename the schema with `{X_RUST_NAME}`")
    };
    return Err(crate::error::Error::TypeNameCollision {
        name: name.logical().to_owned(),
        artifact: artifact.to_owned(),
        hint,
    });
}
