//! Resolution of top-level type names across the lowered IR.
//!
//! A generated type name differs from the plain `to_ident(schema_name)` for two
//! reasons. The schema carries an `x-rust-name` override, or two schema names
//! collapse onto one Rust identifier and the config supplies a suffix. Every
//! reference to a schema lowers to a [`RustType::Named`] that holds the original
//! schema name. Name resolution therefore must also rewrite those references to
//! point at the final identifier. Lowering sets the item names from the same
//! resolution map. See [`crate::lower::schema`]. This pass rewrites the `Named`
//! references that still point at the original name.
//!
//! An unresolved collision is not reported here. Default pruning drops a schema
//! that no operation uses, and a collision between two dropped schemas never
//! reaches the output. [`type_renames`] therefore returns the collisions it found,
//! and the caller reports them when the set of emitted models is final.

use std::collections::HashMap;
use std::collections::HashSet;

use openapiv3::ReferenceOr;

use crate::config::DEFAULT_RESPONSE_SUFFIX;
use crate::config::OUTPUT_OPTIONS_KEY;
use crate::config::RESPONSE_TYPE_SUFFIX_KEY;
use crate::config::TYPE_NAME_SUFFIX_KEY;
use crate::emit::ReservedTypeName;
use crate::error::Error;
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
use crate::naming::RustIdent;
use crate::naming::X_RUST_NAME;
use crate::naming::to_ident;

/// The final Rust type name of every top-level schema, together with each
/// collision that still needs a decision from the author.
///
/// A collision is not an error at resolution time. Default pruning drops a schema
/// that no generated operation uses, and two dropped schemas that collapse onto
/// one Rust identifier cause no problem in the output. The caller therefore holds
/// this value until the module is final and then calls [`TypeNames::check_emitted`].
#[derive(Debug)]
pub struct TypeNames {
    /// A map from an original schema name to the final Rust type identifier.
    renames: HashMap<String, String>,
    /// Every collision found, in document order.
    collisions: Vec<Collision>,
}

/// Two schema names that collapse onto one Rust identifier, and no suffix to
/// tell them apart. The fields are kept instead of a built [`Error`], because the
/// caller decides later whether this collision reaches the output at all.
#[derive(Debug)]
struct Collision {
    /// The Rust identifier that both schemas produce.
    ident: String,
    /// The schema that claimed `ident` first.
    first: String,
    /// The schema that collided with `first`.
    second: String,
    /// Whether `second` carries an `x-rust-name`, which changes the remedy.
    overridden: bool,
}

impl TypeNames {
    /// The renames to apply to every `Named` reference and item name.
    ///
    /// The map holds one entry for each top-level schema whose emitted name
    /// differs from the plain `to_ident(name)`. Two sources add an entry:
    ///
    /// * an `x-rust-name` override (inline schemas only), and
    /// * a collision suffix from `output-options.type-name-suffix`, added to the
    ///   second of two schema names that collapse onto one Rust identifier.
    ///
    /// The map omits a schema whose emitted name does not change. The common case
    /// therefore gives an empty map, and the rewrite passes do no work.
    ///
    /// An unresolved collision adds no entry, so both schemas keep the plain
    /// name. The module then holds two items with that one name, which is why an
    /// unchecked collision must never reach the emitter.
    pub fn renames(&self) -> &HashMap<String, String> {
        return &self.renames;
    }

    /// Report a collision whose Rust type name `module` still holds, and collect
    /// every such collision into one result.
    ///
    /// The emitted name is what decides. Two items with one name do not compile,
    /// whatever schema each item came from, and a schema that `module` does not
    /// hold emits no item at all. The check is therefore keyed on the identifier
    /// and not on the provenance of the two schemas.
    ///
    /// Keying on the identifier also covers a name that a hoisted inline type
    /// takes. Pruning is name-based (see [`crate::lower::prune`]), so an inline
    /// item named `FooBar` keeps two unused `foo-bar` and `fooBar` components
    /// alive. Three items then share one name. The collision is real there, even
    /// though no operation reaches either component.
    ///
    /// A caller that prunes nothing gets a module that holds every schema, so
    /// every collision reports.
    ///
    /// # Errors
    ///
    /// Returns one [`Error::SchemaNameCollision`] for a single surviving
    /// collision, or an [`Error::Validation`] that holds all of them.
    pub fn check_emitted(&self, module: &Module) -> Result<()> {
        let emitted: HashSet<&str> = module.items.iter().map(|item| return item.name()).collect();
        let mut diagnostics = crate::lower::validate::Diagnostics::new();
        for collision in &self.collisions {
            if !emitted.contains(collision.ident.as_str()) {
                continue;
            }
            diagnostics.push(Error::SchemaNameCollision {
                ident: collision.ident.clone(),
                first: collision.first.clone(),
                second: collision.second.clone(),
                hint: collision_hint(&collision.ident, &collision.second, collision.overridden),
            });
        }
        return diagnostics.into_result();
    }
}

/// Resolve the Rust type name of every top-level schema in `spec`.
///
/// Two distinct schema names can collapse onto one Rust identifier. For example,
/// `foo-bar` and `fooBar` both become `FooBar`. When `suffix` is set, the second
/// name takes the suffix. When `suffix` is `None`, the collision is recorded in
/// the returned [`TypeNames`] for the caller to report, because the generator
/// will not choose a name for one of two distinct schemas. That choice belongs to
/// the author.
///
/// # Errors
///
/// A `suffix` that contributes no characters to an identifier is an error.
/// Casing removes punctuation, so a suffix such as `-` leaves the name unchanged
/// and cannot resolve a collision. A valid suffix holds a letter or a digit.
pub fn type_renames(spec: &Spec, suffix: Option<&str>) -> Result<TypeNames> {
    let suffix = checked_suffix(suffix)?;
    let mut resolved = HashMap::new();
    let mut collisions = Vec::new();
    // Maps a claimed identifier back to the schema name that claimed it, so a
    // collision error can name the earlier schema and not the identifier alone.
    let mut claimed: HashMap<String, String> = HashMap::new();
    for (name, entry) in spec.schemas() {
        let override_name = match entry {
            ReferenceOr::Item(schema) => schema
                .schema_data
                .extensions
                .get(X_RUST_NAME)
                .and_then(|value| return value.as_str()),
            ReferenceOr::Reference { .. } => None,
        };
        let effective = override_name.unwrap_or(name);
        let mut ident = to_ident(effective, Case::Pascal);
        if let Some(first) = claimed.get(ident.logical()) {
            match suffix {
                Some(suffix) => {
                    ident = suffixed_ident(&ident, suffix, &claimed);
                }
                None => {
                    collisions.push(Collision {
                        ident: ident.logical().to_owned(),
                        first: first.clone(),
                        second: name.clone(),
                        overridden: override_name.is_some(),
                    });
                    continue;
                }
            }
        }
        claimed.insert(ident.logical().to_owned(), name.clone());
        if ident.logical() != to_ident(name, Case::Pascal).logical() {
            resolved.insert(name.clone(), ident.logical().to_owned());
        }
    }
    return Ok(TypeNames {
        renames: resolved,
        collisions,
    });
}

/// Reject a configured suffix that adds nothing to a Rust type name.
///
/// Casing drops punctuation and separators. `to_ident("Foo -")` therefore gives
/// `Foo` again, and the same holds for an empty suffix. Such a suffix cannot
/// resolve a collision, because the second name stays the same as the first.
/// [`suffixed_ident`] would search for a free name that it can never produce.
///
/// The check runs one time for the whole document, and not for each schema,
/// because the suffix comes from the config and does not change per schema.
///
/// This returns an error and does not treat the suffix as unset. A silent
/// fallback would report a collision and tell the author to set
/// `type-name-suffix`, which the author already did.
fn checked_suffix(suffix: Option<&str>) -> Result<Option<&str>> {
    let Some(suffix) = suffix else {
        return Ok(None);
    };
    // Compare against a fixed stem, because the result must hold for every name.
    // A suffix that adds characters to one name adds them to all names.
    const STEM: &str = "Placeholder";
    if to_ident(&format!("{STEM} {suffix}"), Case::Pascal).logical() != STEM {
        return Ok(Some(suffix));
    }
    return Err(Error::InvalidTypeNameSuffix {
        suffix: suffix.to_owned(),
        hint: format!(
            "Casing removes punctuation and separators, so `{suffix}` leaves the type name unchanged. \
             Use a suffix with at least one letter or digit (for example \
             `{TYPE_NAME_SUFFIX_KEY}: Alt`). To make a collision an error instead, remove \
             `{OUTPUT_OPTIONS_KEY}.{TYPE_NAME_SUFFIX_KEY}`.",
        ),
    });
}

/// Add `suffix` to `ident`, and keep adding it until the result is free.
///
/// Repetition matters for a three-way collision. Two schemas already hold
/// `Widget` and `WidgetAlt`, so a third must not take `WidgetAlt` again.
///
/// The loop ends because `suffix` adds at least one character to the identifier,
/// which [`checked_suffix`] guarantees. Each pass therefore gives a longer name,
/// and the supply of unclaimed names cannot run out.
fn suffixed_ident(ident: &RustIdent, suffix: &str, claimed: &HashMap<String, String>) -> RustIdent {
    let mut candidate = to_ident(&format!("{} {suffix}", ident.logical()), Case::Pascal);
    while claimed.contains_key(candidate.logical()) {
        let longer = to_ident(&format!("{} {suffix}", candidate.logical()), Case::Pascal);
        // Defensive: `checked_suffix` rules this out. Growth is the reason the
        // loop ends, so a non-growing step would spin forever. Stop instead.
        if longer.logical() == candidate.logical() {
            return candidate;
        }
        candidate = longer;
    }
    return candidate;
}

/// Build the remedy text for a schema-name collision.
///
/// `ident` is the Rust identifier that both schemas produce, so the example names
/// real types and not spec names. `second` is the schema that collided.
/// `overridden` records whether `second` already carries an `x-rust-name`. That
/// case needs different advice, because the override itself caused the collision.
fn collision_hint(ident: &str, second: &str, overridden: bool) -> String {
    if overridden {
        return format!(
            "`{second}` already sets `{X_RUST_NAME}`, and that name also resolves to `{ident}`. \
             Give `{second}` a name that no other schema uses.",
        );
    }
    return format!(
        "Give one of the two schemas a different Rust name with `{X_RUST_NAME}`, which records the \
         type name the author wants. To rename every later collision instead, set \
         `{OUTPUT_OPTIONS_KEY}.{TYPE_NAME_SUFFIX_KEY}` (for example `{TYPE_NAME_SUFFIX_KEY}: Alt`, \
         which emits `{ident}` and `{ident}Alt`).",
    );
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
/// Fail generation if a per-operation type name would collide with a
/// component-model name emitted in the same file.
///
/// In the flat layout, component models, per-operation types (response enums,
/// parameter structs, request/response body enums), and the requested generator
/// interfaces (`reserved`, e.g. the `Api` trait or `Client` struct) all share
/// the crate root. A model whose name matches one of those — most commonly a
/// schema named `<Op>Response`, or a schema literally named `Api`/`Client` —
/// would produce two items with the same name. Rather than silently rename,
/// generation fails so the author resolves the clash deliberately: rename the
/// schema with `x-rust-name`, or, for a response-enum clash, set
/// `output-options.response-type-suffix`. Only locally emitted models are
/// considered; import-mapped models are referenced through a qualified path and
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
/// suffix renames *every* response enum, not just the colliding one.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_with_identifier_characters_is_accepted() {
        for suffix in ["Alt", "2", "a", "-v2", "_alt"] {
            let outcome = checked_suffix(Some(suffix));
            assert!(
                matches!(outcome, Ok(Some(kept)) if kept == suffix),
                "`{suffix}` adds characters to a type name and must be accepted",
            );
        }
    }

    #[test]
    fn absent_suffix_stays_absent() {
        assert!(matches!(checked_suffix(None), Ok(None)));
    }

    #[test]
    fn suffix_without_identifier_characters_is_rejected() {
        // Casing drops each of these, so the suffix cannot resolve a collision.
        // An unbounded search for a free name would otherwise never end.
        for suffix in ["", " ", "-", "_", "...", "-_-"] {
            let outcome = checked_suffix(Some(suffix));
            assert!(
                matches!(outcome, Err(Error::InvalidTypeNameSuffix { .. })),
                "`{suffix}` adds nothing to a type name and must be rejected",
            );
        }
    }

    #[test]
    fn rejected_suffix_names_both_remedies() {
        let Err(err) = checked_suffix(Some("-")) else {
            panic!("`-` must be rejected");
        };
        let Error::InvalidTypeNameSuffix { hint, .. } = &err else {
            panic!("expected an InvalidTypeNameSuffix, got {err:?}");
        };
        // The hint must show how to make the suffix work, and how to go back to
        // an error for a collision.
        assert!(hint.contains(TYPE_NAME_SUFFIX_KEY), "hint names the key: {hint}");
        assert!(hint.contains("Alt"), "hint gives a working example: {hint}");
        assert!(hint.contains("remove"), "hint offers the error mode: {hint}");
        // The message states the problem. The console prints the hint under it,
        // so `Display` must not repeat the hint.
        assert!(!err.to_string().contains(hint.as_str()));
    }

    #[test]
    fn suffixed_ident_grows_until_the_name_is_free() {
        let mut claimed: HashMap<String, String> = HashMap::new();
        claimed.insert("OrderItem".to_owned(), "order-item".to_owned());
        claimed.insert("OrderItemAlt".to_owned(), "orderItem".to_owned());
        // `OrderItem` and `OrderItemAlt` are taken, so a third collision must
        // repeat the suffix instead of reusing `OrderItemAlt`.
        let ident = suffixed_ident(&to_ident("order-item", Case::Pascal), "Alt", &claimed);
        assert_eq!(ident.logical(), "OrderItemAltAlt");
    }
}
