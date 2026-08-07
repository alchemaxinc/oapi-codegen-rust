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
use crate::emit::Targets;
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
        RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) | RustType::Boxed(inner) => {
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
        RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) | RustType::Boxed(inner) => {
            rewrite_type(inner, renames);
        }
        _ => {}
    }
}
/// Fail generation if two items of `module` take one Rust type name.
///
/// [`TypeNames::check_emitted`] covers two component schemas that collapse onto
/// one identifier. It cannot cover a hoisted inline schema, because such a schema
/// has no name in `components` for the resolution pass to see. Lowering names a
/// hoisted item after the property path that encloses it, so a component schema
/// named after that same path takes the same name. The emitted item is what
/// decides, so this check reads the final item names.
///
/// Every generation mode calls this check, including models-only generation.
///
/// # Errors
///
/// Returns one [`Error::DuplicateTypeName`] for a single duplicate name, or an
/// [`Error::Validation`] that holds all of them.
pub fn check_duplicate_models(module: &Module) -> Result<()> {
    let mut diagnostics = crate::lower::validate::Diagnostics::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for item in &module.items {
        if !seen.insert(item.name()) {
            diagnostics.push(Error::DuplicateTypeName {
                name: item.name().to_owned(),
                hint: duplicate_model_hint(item.name()),
            });
        }
    }
    return diagnostics.into_result();
}

/// Fail generation if an emitted item takes the name of a prelude type that the
/// file names without a path.
///
/// This is not a duplicate-name check. A schema named `Option` emits one item,
/// so [`check_duplicate_models`] and [`check_type_name_collisions`] both pass.
/// The item shadows `Option` for the whole file instead, and every `Option<T>`
/// in it then reads as that struct.
///
/// Every generation mode calls this check. `targets` says which names to hold,
/// because only a server or a client writes `Result`.
///
/// The check reads names, not uses, so it is wider than it has to be. A spec
/// with a schema named `Box` and no recursion writes no `Box<T>`, and it would
/// compile. It is still rejected. Two reasons keep it that way: a false
/// rejection is loud and has a one-line remedy in the hint, while a missed use
/// site emits code that does not compile, which is the failure this check
/// exists to stop. The verdict also stays put. Adding a recursive schema later
/// cannot turn an accepted name into a broken build.
///
/// # Errors
///
/// Returns one [`Error::PreludeShadowing`] for a single name, or an
/// [`Error::Validation`] that holds all of them.
pub fn check_prelude_shadowing(module: &Module, targets: Targets) -> Result<()> {
    let mut diagnostics = crate::lower::validate::Diagnostics::new();
    let prelude = crate::emit::prelude_type_names(targets);
    for item in &module.items {
        let Some(shadowed) = prelude.iter().find(|entry| return entry.name == item.name()) else {
            continue;
        };
        diagnostics.push(Error::PreludeShadowing {
            name: shadowed.name.to_owned(),
            used_for: shadowed.used_for.to_owned(),
            hint: format!("Rename the schema with `{X_RUST_NAME}`, or with `output-options.type-name-suffix`."),
        });
    }
    return diagnostics.into_result();
}

/// What claimed one crate-root type name.
enum Claim {
    /// A component model, or an inline schema that lowering hoisted to the crate
    /// root. The two are one case here, because the emitted item is the same kind
    /// of item and the remedy is the same.
    Model,
    /// A fixed type name that a requested generator interface emits, for example
    /// the `Api` trait. The payload describes what emits it.
    Reserved(&'static str),
    /// A per-operation type. Every such name derives from the method name of the
    /// operation that produced it, so the remedy names that operation.
    Artifact {
        /// What the generator emits, for example `query-parameter struct`.
        kind: &'static str,
        /// The `Api` method name of the operation that produced it.
        operation: String,
    },
}

/// Fail generation if two items that the file holds take one Rust type name.
///
/// In the flat layout, component models, inline schemas that lowering hoists to
/// the crate root, per-operation types (response enums, parameter structs, and
/// request/response body enums), and the requested generator interfaces
/// (`reserved`, for example the `Api` trait or the `Client` struct) all share the
/// crate root. Any two of them that take one name emit two items with that name,
/// which does not compile. The check therefore holds one namespace and reports
/// every claim that some earlier claim already took. Four cases reach it.
///
/// * A model against a reserved name. A component schema or a hoisted inline
///   schema named `Api` or `Client` is the case.
/// * A per-operation type against a model. The most common case is a schema named
///   `<Op>Response`.
/// * A per-operation type against a reserved name. An operation named `api` gives
///   a response enum named `Api` when the suffix adds no characters.
/// * A per-operation type against another per-operation type. A
///   `response-type-suffix` that matches a parameter-struct suffix is one way to
///   reach this, because it makes one operation's response enum take the name of
///   a parameter struct.
///
/// Two models that take one name are the fifth pair in this namespace, and
/// [`check_duplicate_models`] reports them. This check seeds the namespace with
/// every model name and reports nothing for a repeat, so one clash gives one
/// problem. That split needs the caller to run [`check_duplicate_models`] as
/// well, which every generation mode does.
///
/// Rather than rename an item, generation fails, so the author resolves the clash.
/// The `hint` of each problem names the remedy for that case.
///
/// Only locally emitted models count. An import-mapped model is referenced
/// through a qualified path and cannot collide with a crate-root type.
///
/// # Errors
///
/// Returns one collision error for a single clash, or an [`Error::Validation`]
/// that holds all of them. One run therefore reports every clash it finds.
pub fn check_type_name_collisions(service: &Service, module: &Module, reserved: &[ReservedTypeName]) -> Result<()> {
    let mut diagnostics = crate::lower::validate::Diagnostics::new();
    // Seeding records each model name and reports nothing for a repeat, because
    // `check_duplicate_models` owns that pair. This check does not enforce that the
    // caller runs it. A caller that skips it emits two items with one name and no
    // error, so every generation mode must call both.
    let mut claimed: HashMap<String, Claim> = module
        .items
        .iter()
        .map(|item| return (item.name().to_owned(), Claim::Model))
        .collect();
    for name in reserved {
        // A model that took a reserved name keeps its `Claim::Model` entry, so the
        // problem reports once here and not again for the reserved claim.
        match claimed.insert(name.name.to_owned(), Claim::Reserved(name.description)) {
            Some(Claim::Model) => {
                claimed.insert(name.name.to_owned(), Claim::Model);
                diagnostics.push(Error::TypeNameCollision {
                    name: name.name.to_owned(),
                    artifact: name.description.to_owned(),
                    hint: format!("rename the schema with `{X_RUST_NAME}`"),
                });
            }
            Some(Claim::Reserved(_) | Claim::Artifact { .. }) | None => {}
        }
    }
    for operation in &service.operations {
        let mut claim = |name: &RustIdent, kind: &'static str| {
            claim_artifact(&mut claimed, &mut diagnostics, name, kind, &operation.name);
        };
        claim(&operation.response_enum, "response enum");
        if let Some(query) = &operation.query {
            claim(&query.name, "query-parameter struct");
        }
        if let Some(headers) = &operation.headers {
            claim(&headers.name, "header-parameter struct");
        }
        if let Some(cookies) = &operation.cookies {
            claim(&cookies.name, "cookie-parameter struct");
        }
        match &operation.request {
            Some(RequestPayload::Multipart(multipart)) => claim(&multipart.name, "multipart request struct"),
            Some(RequestPayload::Negotiated(request)) => claim(&request.name, "request-body enum"),
            Some(RequestPayload::Single(_)) | None => {}
        }
        for response in &operation.responses {
            if let Some(ResponseBody::Negotiated(body)) = &response.body {
                claim(&body.name, "response-body enum");
            }
        }
    }
    return diagnostics.into_result();
}

/// Record one per-operation type name in `claimed`, or report the clash when some
/// earlier claim already took it.
fn claim_artifact(
    claimed: &mut HashMap<String, Claim>,
    diagnostics: &mut crate::lower::validate::Diagnostics,
    name: &RustIdent,
    kind: &'static str,
    operation: &RustIdent,
) {
    let ident = name.logical();
    match claimed.get(ident) {
        None => {
            claimed.insert(
                ident.to_owned(),
                Claim::Artifact {
                    kind,
                    operation: operation.logical().to_owned(),
                },
            );
        }
        Some(Claim::Model) => {
            diagnostics.push(Error::TypeNameCollision {
                name: ident.to_owned(),
                artifact: kind.to_owned(),
                hint: model_clash_hint(kind),
            });
        }
        Some(Claim::Reserved(description)) => {
            diagnostics.push(Error::OperationTypeCollision {
                name: ident.to_owned(),
                first: format!("the {description}"),
                second: format!("the {kind} of operation `{}`", operation.logical()),
                hint: reserved_clash_hint(operation.logical()),
            });
        }
        Some(Claim::Artifact {
            kind: first_kind,
            operation: first_operation,
        }) => {
            diagnostics.push(Error::OperationTypeCollision {
                name: ident.to_owned(),
                first: format!("the {first_kind} of operation `{first_operation}`"),
                second: format!("the {kind} of operation `{}`", operation.logical()),
                hint: artifact_clash_hint(first_operation, operation.logical()),
            });
        }
    }
}

/// The remedy for two models that take one name.
///
/// At least one of the two is a hoisted inline schema, which carries no name of
/// its own to override. The remedy therefore acts on the component schema that
/// encloses it, or removes the hoist by giving the inline schema a component of
/// its own.
fn duplicate_model_hint(name: &str) -> String {
    return format!(
        "One of these comes from an inline schema that the generator hoists to the crate root, and \
         an inline schema carries no name to override. Give the enclosing component schema a \
         different Rust name with `{X_RUST_NAME}`, or move the inline schema into its own component \
         schema, name that component something other than `{name}`, and refer to it with `$ref`.",
    );
}

/// The remedy for a per-operation type that takes the name of an emitted model.
///
/// A response-enum clash has a second remedy, because one config key renames
/// every response enum. The hint leads with the per-schema `x-rust-name` fix,
/// which leaves the other response enums untouched, and offers the broad suffix
/// after it.
fn model_clash_hint(kind: &str) -> String {
    if kind != "response enum" {
        return format!("rename the schema with `{X_RUST_NAME}`");
    }
    let default_suffix = to_ident(DEFAULT_RESPONSE_SUFFIX, Case::Pascal);
    return format!(
        "give the colliding schema a different Rust name with `{X_RUST_NAME}` — a surgical, \
         per-schema fix that leaves the other response enums untouched — or, to rename every \
         response enum, set `{OUTPUT_OPTIONS_KEY}.{RESPONSE_TYPE_SUFFIX_KEY}` to a suffix other \
         than the default `{}` (for example `{RESPONSE_TYPE_SUFFIX_KEY}: Resp`, which renames \
         the enum to `<Op>Resp`)",
        default_suffix.logical(),
    );
}

/// The remedy for a per-operation type that takes the name of a generator
/// interface.
///
/// The interface name is fixed, so only the operation side can move. Every
/// per-operation type derives from the method name, so `x-rust-name` on the
/// operation resolves the clash. A configured suffix can also produce this name,
/// which the second remedy covers.
fn reserved_clash_hint(operation: &str) -> String {
    return format!(
        "The generator emits this name for a requested target, so the name cannot move. Give \
         operation `{operation}` a different method name with `{X_RUST_NAME}`, or change \
         `{OUTPUT_OPTIONS_KEY}.{RESPONSE_TYPE_SUFFIX_KEY}` if that suffix produced the name.",
    );
}

/// The remedy for two per-operation types that take one name.
///
/// One operation that produces both names is a different problem from two
/// operations that produce one name. No method name can separate two types of one
/// operation, so that case names the suffix that made them equal. Two operations
/// take the per-operation `x-rust-name` remedy, because every per-operation type
/// derives from the method name.
fn artifact_clash_hint(first_operation: &str, second_operation: &str) -> String {
    if first_operation == second_operation {
        return format!(
            "Both names belong to operation `{first_operation}`, so no method name can separate \
             them. Set `{OUTPUT_OPTIONS_KEY}.{RESPONSE_TYPE_SUFFIX_KEY}` to a suffix that no \
             parameter-struct or body-enum name already ends with.",
        );
    }
    return format!(
        "Every per-operation type derives from the method name of its operation. Give operation \
         `{first_operation}` or operation `{second_operation}` a different method name with \
         `{X_RUST_NAME}`.",
    );
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
