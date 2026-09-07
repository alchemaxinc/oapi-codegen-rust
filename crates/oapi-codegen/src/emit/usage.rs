//! Deciding each generated model's serde derive set from how the API uses it.
//!
//! A generated type only needs a serde trait in the direction its API position
//! exercises: a server serializes response bodies and deserializes request
//! bodies. a client does the reverse. Deriving a trait a type never needs will
//! impose an unsatisfiable bound on a reused `x-rust-type` target — for example forcing
//! `Deserialize` on a response-only type a project only ever serializes.
//!
//! This module walks the [`Service`] to seed each component model as
//! request-reachable and/or response-reachable, propagates that reachability
//! through inter-model references, and combines it with the generation
//! [`Targets`] to pick each model's [`SerdeDerives`]. A model used in both
//! directions — or any model when both a server and a client are generated —
//! derives both traits, exactly as the generator did unconditionally before.

use std::collections::HashMap;

use crate::emit::Targets;
use crate::emit::models::ModelDerives;
use crate::emit::models::SerdeDerives;
use crate::ir::Direction;
use crate::ir::ForeignDerives;
use crate::ir::Item;
use crate::ir::Module;
use crate::ir::NegotiatedBody;
use crate::ir::RequestPayload;
use crate::ir::ResponseBody;
use crate::ir::RustType;
use crate::ir::Service;
use crate::ir::Struct;
use crate::naming::Case;
use crate::naming::to_ident;

/// Whether a model is reachable as a request payload and/or a response payload.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Usage {
    /// Reachable from a request body or request-input struct.
    pub(crate) request: bool,
    /// Reachable from a response body.
    pub(crate) response: bool,
}

/// Compute the derive set for every generated model, keyed by its logical name.
///
/// A name absent from the returned map is unreferenced by any operation (for
/// example under `skip-prune`) and derives everything.
pub(crate) fn model_derives(module: &Module, service: &Service, targets: Targets) -> HashMap<String, ModelDerives> {
    let adjacency = adjacency(module);
    let foreign = foreign_derives(module, &adjacency);
    let usage = direction_usage(module, service);

    // Union of both keys. A model can be constrained by a foreign type without
    // being reachable from any operation, and the other way round, so taking only
    // the usage keys would drop the foreign narrowing for an unreferenced model.
    let mut names: Vec<String> = usage.keys().cloned().collect();
    for name in foreign.keys() {
        if !usage.contains_key(name) {
            names.push(name.clone());
        }
    }

    return names
        .into_iter()
        .map(|name| {
            let used = usage.get(&name).copied().unwrap_or_default();
            let serde = SerdeDerives {
                serialize: (targets.server && used.response) || (targets.client && used.request),
                deserialize: (targets.server && used.request) || (targets.client && used.response),
            };
            // Absent means unconstrained, because only a model reaching a
            // restricted foreign type gets an entry.
            let constraint = foreign.get(&name).copied().unwrap_or_default();
            let derives = ModelDerives {
                serde,
                foreign: constraint,
            };
            return (name, derives);
        })
        .collect();
}

/// The derive set for every model when only models are generated, with no service
/// to say which direction each one travels in.
///
/// Every model keeps both serde traits, because an unknown direction cannot narrow
/// them. Only the models reaching a restricted foreign type get an entry, so an
/// absent name means the unconstrained default.
pub(crate) fn models_only_derives(module: &Module) -> HashMap<String, ModelDerives> {
    let adjacency = adjacency(module);
    return foreign_derives(module, &adjacency)
        .into_iter()
        .map(|(name, foreign)| {
            return (
                name,
                ModelDerives {
                    serde: SerdeDerives::both(),
                    foreign,
                },
            );
        })
        .collect();
}

/// A resolved answer to "which of `Debug`, `Clone`, `PartialEq` may this type
/// carry", for any type expression the emitters build.
///
/// A per-operation type is not a component model, so it gets no entry in the
/// model map. It still holds model types — a response enum carries the response
/// bodies — and a derive on it is only as satisfiable as those bodies. Emitting
/// `Clone` on a response enum whose body model had to drop `Clone` produces
/// generated code that does not compile, which is the failure mode this whole
/// feature exists to prevent. So the per-operation emitters resolve through this
/// as well.
#[derive(Debug, Default)]
pub(crate) struct ForeignResolver {
    /// Per model, what it may derive. An absent model is unconstrained.
    models: HashMap<String, ForeignDerives>,
}

impl ForeignResolver {
    /// What one type expression allows, resolving a model reference through the
    /// map and looking through the `Vec`, `Map`, and `Option` wrappers.
    pub(crate) fn of_type(&self, ty: &RustType) -> ForeignDerives {
        return match ty {
            RustType::Verbatim { derives, .. } => *derives,
            RustType::Named(name) => {
                let canonical = to_ident(name, Case::Pascal);
                return self.models.get(canonical.logical()).copied().unwrap_or_default();
            }
            RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) | RustType::Boxed(inner) => {
                self.of_type(inner)
            }
            _ => ForeignDerives::default(),
        };
    }

    /// What a group of type expressions allows together, which is what all of
    /// them allow.
    pub(crate) fn of_types<'a>(&self, types: impl IntoIterator<Item = &'a RustType>) -> ForeignDerives {
        let mut allowed = ForeignDerives::default();
        for ty in types {
            allowed = allowed.intersect(self.of_type(ty));
        }
        return allowed;
    }
}

/// The non-serde traits each model can derive, for every model a restricted
/// foreign type constrains. A model absent from the map is unconstrained.
///
/// A foreign type constrains the model that names it and every model that reaches
/// that one, because a derive is only as satisfiable as the fields it recurses
/// through. So the constraint propagates along the same reference graph the
/// direction walk uses, but the other way down it: direction flows from an
/// operation towards the leaves, and a trait bound flows from a leaf back up
/// towards whatever holds it.
pub(crate) fn foreign_resolver(module: &Module) -> ForeignResolver {
    let adjacency = adjacency(module);
    return ForeignResolver {
        models: foreign_derives(module, &adjacency),
    };
}

/// The constrained-model map [`ForeignResolver`] wraps.
fn foreign_derives(module: &Module, adjacency: &HashMap<String, Vec<String>>) -> HashMap<String, ForeignDerives> {
    // Seed each model with what its own directly-named foreign types allow.
    let mut direct: HashMap<String, ForeignDerives> = HashMap::new();
    let raw_unions: std::collections::HashSet<&str> = module
        .items
        .iter()
        .filter_map(|item| {
            if matches!(item, Item::Enum(enom) if matches!(enom.kind, crate::ir::EnumKind::AnyOf(_))) {
                return Some(item.name());
            }
            return None;
        })
        .collect();
    for item in &module.items {
        if raw_unions.contains(item.name()) {
            continue;
        }
        let mut allowed = ForeignDerives::default();
        for ty in item_types(item) {
            allowed = allowed.intersect(direct_foreign_constraint(&ty));
        }
        if !allowed.is_unconstrained() {
            direct.insert(item.name().to_owned(), allowed);
        }
    }
    if direct.is_empty() {
        return HashMap::new();
    }

    // Reverse the reference graph so a constraint can be pushed from a referenced
    // model to the models naming it.
    let mut referrers: HashMap<&str, Vec<&str>> = HashMap::new();
    for (name, targets) in adjacency {
        if raw_unions.contains(name.as_str()) {
            continue;
        }
        for target in targets {
            referrers.entry(target.as_str()).or_default().push(name.as_str());
        }
    }

    // Propagate to a fixed point. Each push can only clear a flag, never set one,
    // so a model is re-visited at most once per flag and the walk terminates.
    let mut resolved = direct.clone();
    let mut stack: Vec<String> = direct.keys().cloned().collect();
    while let Some(name) = stack.pop() {
        let constraint = match resolved.get(&name) {
            Some(constraint) => *constraint,
            None => continue,
        };
        let Some(parents) = referrers.get(name.as_str()) else {
            continue;
        };
        for parent in parents {
            let current = resolved.get(*parent).copied().unwrap_or_default();
            let merged = current.intersect(constraint);
            if merged != current {
                resolved.insert((*parent).to_owned(), merged);
                stack.push((*parent).to_owned());
            }
        }
    }
    return resolved;
}

/// What a single type expression allows, counting only the foreign types it names
/// directly.
///
/// Every wrapper derives all three when its element does, so `Vec`, `Map`, and
/// `Option` are transparent. A [`RustType::Named`] contributes nothing here, and
/// deliberately: this function seeds the graph walk, which is what carries a
/// constraint across a model reference. Resolving `Named` here as well would be
/// the same answer reached twice, and only for depth one.
fn direct_foreign_constraint(ty: &RustType) -> ForeignDerives {
    return match ty {
        RustType::Verbatim { derives, .. } => *derives,
        RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) | RustType::Boxed(inner) => {
            direct_foreign_constraint(inner)
        }
        _ => ForeignDerives::default(),
    };
}

/// Every type expression an item holds: its field types, its variant types, or
/// its alias target.
///
/// The alias target is included even though an alias emits no derive of its own.
/// A model naming the alias reaches the foreign type through it, and the walk
/// finds that only if the alias reports the constraint.
fn item_types(item: &Item) -> Vec<RustType> {
    let mut types = Vec::new();
    match item {
        Item::Struct(strukt) => {
            for field in &strukt.fields {
                types.push(field.ty.clone());
            }
            if let Some(additional) = &strukt.additional_properties {
                types.push(additional.clone());
            }
        }
        Item::Enum(enom) => {
            if let crate::ir::EnumKind::Union(variants) | crate::ir::EnumKind::AnyOf(variants) = &enom.kind {
                for variant in variants {
                    types.push(variant.ty.clone());
                }
            }
        }
        Item::Alias(alias) => types.push(alias.ty.clone()),
    }
    return types;
}

/// Which direction reaches each model, keyed by its logical name.
///
/// An absent name is reached by no operation, which happens under `skip-prune`.
pub(crate) fn direction_usage(module: &Module, service: &Service) -> HashMap<String, Usage> {
    let adjacency = adjacency(module);
    let mut usage: HashMap<String, Usage> = HashMap::new();
    for name in request_seeds(service) {
        mark(&adjacency, &name, &mut usage, Direction::Request);
    }
    for name in response_seeds(service) {
        mark(&adjacency, &name, &mut usage, Direction::Response);
    }
    return usage;
}

/// Mark `start` and every model reachable from it with `direction`, following
/// inter-model references until no new model is reached.
fn mark(
    adjacency: &HashMap<String, Vec<String>>,
    start: &str,
    usage: &mut HashMap<String, Usage>,
    direction: Direction,
) {
    let mut stack = vec![start.to_owned()];
    while let Some(name) = stack.pop() {
        let entry = usage.entry(name.clone()).or_default();
        let already = match direction {
            Direction::Request => entry.request,
            Direction::Response => entry.response,
        };
        if already {
            continue;
        }
        match direction {
            Direction::Request => entry.request = true,
            Direction::Response => entry.response = true,
        }
        if let Some(neighbours) = adjacency.get(&name) {
            stack.extend(neighbours.iter().cloned());
        }
    }
}

/// Build the model-reference graph: each item name mapped to the names of the
/// generated models it references through its fields, variants, or alias target.
pub(crate) fn adjacency(module: &Module) -> HashMap<String, Vec<String>> {
    let mut graph = HashMap::with_capacity(module.items.len());
    for item in &module.items {
        graph.insert(item.name().to_owned(), item_references(item));
    }
    return graph;
}

/// The generated-model names an item references.
fn item_references(item: &Item) -> Vec<String> {
    let mut names = Vec::new();
    match item {
        Item::Struct(strukt) => {
            for field in &strukt.fields {
                collect_named(&field.ty, &mut names);
            }
            if let Some(additional) = &strukt.additional_properties {
                collect_named(additional, &mut names);
            }
        }
        Item::Enum(enom) => {
            if let crate::ir::EnumKind::Union(variants) | crate::ir::EnumKind::AnyOf(variants) = &enom.kind {
                for variant in variants {
                    collect_named(&variant.ty, &mut names);
                }
            }
        }
        Item::Alias(alias) => collect_named(&alias.ty, &mut names),
    }
    return names;
}

/// Collect every [`RustType::Named`] reachable within `ty` (through `Vec`,
/// `Map`, and `Option` wrappers), canonicalized to the PascalCase identifier the
/// emitter and item names use. A bare `Named` still holds the original schema
/// name (the rename pass only rewrites `x-rust-name`/collision cases), so the
/// name is run through [`to_ident`] to match `Item::name` — otherwise a
/// non-PascalCase schema name will fail the usage lookup and fall back to
/// deriving both serde traits. External and verbatim types name no generated
/// model, so they contribute nothing.
fn collect_named(ty: &RustType, out: &mut Vec<String>) {
    match ty {
        RustType::Named(name) => out.push(to_ident(name, Case::Pascal).logical().to_owned()),
        RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) | RustType::Boxed(inner) => {
            collect_named(inner, out)
        }
        _ => {}
    }
}

/// Model names seeded as request-reachable: path/query/header/cookie parameters
/// plus the request payload. A path parameter is deserialized by the server's
/// `Path` extractor, so a schema-typed parameter (for example a component enum) imposes
/// a real `Deserialize` bound. header/cookie parameters are seeded too so a
/// model used only there is narrowed to the request direction rather than
/// falling back to both traits.
fn request_seeds(service: &Service) -> Vec<String> {
    let mut names = Vec::new();
    for operation in &service.operations {
        for param in &operation.path_params {
            collect_named(&param.ty, &mut names);
        }
        if let Some(query) = &operation.query {
            struct_field_names(query, &mut names);
        }
        if let Some(headers) = &operation.headers {
            for param in &headers.params {
                collect_named(&param.ty, &mut names);
            }
        }
        if let Some(cookies) = &operation.cookies {
            for param in &cookies.params {
                collect_named(&param.ty, &mut names);
            }
        }
        if let Some(request) = &operation.request {
            match request {
                RequestPayload::Single(body) => collect_named(&body.ty, &mut names),
                RequestPayload::Multipart(multipart) => {
                    for field in &multipart.fields {
                        collect_named(&field.ty, &mut names);
                    }
                }
                RequestPayload::Negotiated(body) => negotiated_names(body, &mut names),
            }
        }
    }
    return names;
}

/// Model names seeded as response-reachable: every response variant's body and
/// its declared response headers.
fn response_seeds(service: &Service) -> Vec<String> {
    let mut names = Vec::new();
    for operation in &service.operations {
        for case in &operation.responses {
            for header in &case.headers {
                collect_named(&header.ty, &mut names);
            }
            match &case.body {
                Some(ResponseBody::Single(body)) => collect_named(&body.ty, &mut names),
                Some(ResponseBody::Negotiated(body)) => negotiated_names(body, &mut names),
                None => {}
            }
        }
    }
    return names;
}

/// Collect the model names referenced by a negotiated (multi-content-type) body.
fn negotiated_names(body: &NegotiatedBody, out: &mut Vec<String>) {
    for variant in &body.variants {
        collect_named(&variant.body.ty, out);
    }
}

/// Collect the model names referenced by a struct's fields.
fn struct_field_names(strukt: &Struct, out: &mut Vec<String>) {
    for field in &strukt.fields {
        collect_named(&field.ty, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Access;
    use crate::ir::Alias;
    use crate::ir::Body;
    use crate::ir::Field;
    use crate::ir::Operation;
    use crate::ir::Param;
    use crate::ir::ResponseCase;
    use crate::ir::ResponseHeader;
    use crate::ir::ResponseStatus;
    use crate::naming::RustIdent;

    fn tname(name: &str) -> RustIdent {
        return to_ident(name, Case::Pascal);
    }

    fn fname(name: &str) -> RustIdent {
        return to_ident(name, Case::Snake);
    }

    fn named_field(field: &str, ty: &str) -> Field {
        return Field {
            name: fname(field),
            rename: None,
            doc: None,
            deprecated: None,
            ty: RustType::Named(ty.to_owned()),
            required: true,
            omit_empty: None,
            serde_skip: false,
            default: None,
            constraints: None,
            access: Access::ReadWrite,
        };
    }

    fn strukt(name: &str, fields: Vec<Field>) -> Item {
        return Item::Struct(Struct {
            name: tname(name),
            doc: None,
            deprecated: None,
            fields,
            additional_properties: None,
            deny_unknown_fields: false,
        });
    }

    fn response_operation(body_ty: &str) -> Operation {
        return Operation {
            name: fname("get_thing"),
            response_enum: tname("GetThingResponse"),
            doc: None,
            method: "get".to_owned(),
            path: "/thing".to_owned(),
            path_params: Vec::new(),
            query: None,
            headers: None,
            cookies: None,
            request: None,
            responses: vec![ResponseCase {
                variant: tname("Ok"),
                status: ResponseStatus::Fixed(200),
                body: Some(ResponseBody::Single(Body {
                    ty: RustType::Named(body_ty.to_owned()),
                    kind: crate::ir::BodyKind::Json,
                })),
                headers: Vec::new(),
                doc: None,
            }],
            security: Vec::new(),
        };
    }

    /// A struct field holding an `x-rust-type` target with the given declaration.
    fn foreign_field(field: &str, text: &str, derives: ForeignDerives) -> Field {
        let mut field = named_field(field, "Placeholder");
        field.ty = RustType::Verbatim {
            text: text.to_owned(),
            derives,
        };
        return field;
    }

    const DEBUG_ONLY: ForeignDerives = ForeignDerives {
        debug: true,
        clone: false,
        partial_eq: false,
    };

    const NO_PARTIAL_EQ: ForeignDerives = ForeignDerives {
        debug: true,
        clone: true,
        partial_eq: false,
    };

    /// The resolved constraint on `name`, for a module built from `items`.
    fn constraint_of(items: Vec<Item>, name: &str) -> ForeignDerives {
        let module = Module { items };
        let adjacency = adjacency(&module);
        return foreign_derives(&module, &adjacency)
            .get(name)
            .copied()
            .unwrap_or_default();
    }

    #[test]
    fn model_naming_a_restricted_target_loses_the_missing_traits() {
        let items = vec![strukt(
            "Holder",
            vec![foreign_field("value", "crate::Opaque", DEBUG_ONLY)],
        )];
        assert_eq!(constraint_of(items, "Holder"), DEBUG_ONLY);
    }

    #[test]
    fn constraint_propagates_up_one_reference() {
        // The constraint travels from the model naming the target to the model
        // naming that one, which is the direction a trait bound flows.
        let items = vec![
            strukt("Holder", vec![foreign_field("value", "crate::Opaque", DEBUG_ONLY)]),
            strukt("Outer", vec![named_field("held", "Holder")]),
        ];
        assert_eq!(constraint_of(items, "Outer"), DEBUG_ONLY);
    }

    #[test]
    fn constraint_propagates_through_an_alias() {
        // `type X = Y;` derives nothing itself, so the alias is only a hop the walk
        // has to cross rather than a model the constraint stops at.
        let items = vec![
            Item::Alias(Alias {
                name: tname("Opaque"),
                doc: None,
                deprecated: None,
                ty: RustType::Verbatim {
                    text: "crate::Opaque".to_owned(),
                    derives: DEBUG_ONLY,
                },
            }),
            strukt("Holder", vec![named_field("value", "Opaque")]),
        ];
        assert_eq!(constraint_of(items, "Holder"), DEBUG_ONLY);
    }

    #[test]
    fn two_restricted_targets_intersect_rather_than_the_last_one_winning() {
        let items = vec![strukt(
            "Holder",
            vec![
                foreign_field("opaque", "crate::Opaque", DEBUG_ONLY),
                foreign_field("handle", "crate::Handle", NO_PARTIAL_EQ),
            ],
        )];
        // `Debug` is all both declarations have in common.
        assert_eq!(constraint_of(items, "Holder"), DEBUG_ONLY);
    }

    #[test]
    fn wrappers_are_transparent_to_the_constraint() {
        let mut field = foreign_field("values", "crate::Opaque", DEBUG_ONLY);
        field.ty = RustType::Vec(Box::new(RustType::Option(Box::new(field.ty.clone()))));
        assert_eq!(constraint_of(vec![strukt("Holder", vec![field])], "Holder"), DEBUG_ONLY);
    }

    #[test]
    fn model_reaching_no_foreign_type_keeps_every_trait() {
        let items = vec![strukt("Plain", vec![named_field("name", "Other")])];
        let module = Module { items };
        let adjacency = adjacency(&module);
        assert!(
            foreign_derives(&module, &adjacency).is_empty(),
            "an unconstrained model needs no entry at all"
        );
    }

    #[test]
    fn a_reference_cycle_terminates() {
        // Two models naming each other. The walk stops because each push can only
        // clear a flag and never set one.
        let items = vec![
            strukt(
                "Left",
                vec![
                    foreign_field("value", "crate::Opaque", DEBUG_ONLY),
                    named_field("right", "Right"),
                ],
            ),
            strukt("Right", vec![named_field("left", "Left")]),
        ];
        assert_eq!(constraint_of(items.clone(), "Left"), DEBUG_ONLY);
        assert_eq!(constraint_of(items, "Right"), DEBUG_ONLY);
    }

    #[test]
    fn resolver_narrows_a_per_operation_type_through_a_model_name() {
        // A per-operation type gets no entry in the model map, so the resolver has
        // to reach the answer through the models the type holds.
        let module = Module {
            items: vec![strukt(
                "Holder",
                vec![foreign_field("value", "crate::Opaque", DEBUG_ONLY)],
            )],
        };
        let resolver = foreign_resolver(&module);
        assert_eq!(resolver.of_type(&RustType::Named("Holder".to_owned())), DEBUG_ONLY);
        assert_eq!(
            resolver.of_types([&RustType::Named("Holder".to_owned()), &RustType::String].into_iter()),
            DEBUG_ONLY,
            "a primitive alongside a constrained model does not widen the answer"
        );
    }

    #[test]
    fn response_only_model_on_server_skips_deserialize() {
        let module = Module {
            items: vec![
                strukt("Thing", vec![named_field("nested", "Nested")]),
                strukt("Nested", vec![]),
            ],
        };
        let service = Service {
            operations: vec![response_operation("Thing")],
            security_schemes: Vec::new(),
        };
        let targets = Targets {
            server: true,
            client: false,
        };
        let derives = model_derives(&module, &service, targets);

        for name in ["Thing", "Nested"] {
            let serde = derives
                .get(name)
                .map(|derives| return derives.serde)
                .expect("model reached");
            assert!(serde.serialize, "{name} is serialized on the server");
            assert!(
                !serde.deserialize,
                "{name} is never deserialized on a response-only server"
            );
        }
    }

    #[test]
    fn response_only_model_with_client_keeps_both() {
        let module = Module {
            items: vec![strukt("Thing", vec![])],
        };
        let service = Service {
            operations: vec![response_operation("Thing")],
            security_schemes: Vec::new(),
        };
        let targets = Targets {
            server: true,
            client: true,
        };
        let derives = model_derives(&module, &service, targets);
        let serde = derives
            .get("Thing")
            .map(|derives| return derives.serde)
            .expect("model reached");
        assert!(serde.serialize);
        assert!(serde.deserialize, "a client deserializes the response body");
    }

    #[test]
    fn alias_reference_propagates_direction() {
        let module = Module {
            items: vec![
                strukt("Thing", vec![named_field("floor", "FloorHeating")]),
                Item::Alias(Alias {
                    name: tname("FloorHeating"),
                    doc: None,
                    deprecated: None,
                    ty: RustType::verbatim("crate::domain::FloorHeating"),
                }),
            ],
        };
        let service = Service {
            operations: vec![response_operation("Thing")],
            security_schemes: Vec::new(),
        };
        let targets = Targets {
            server: true,
            client: false,
        };
        let derives = model_derives(&module, &service, targets);
        let serde = derives
            .get("FloorHeating")
            .map(|derives| return derives.serde)
            .expect("alias reached");
        assert!(serde.serialize);
        assert!(!serde.deserialize);
    }

    #[test]
    fn non_pascal_schema_name_is_matched_after_canonicalization() {
        // The referencing field carries the raw schema name (`floor_heating`),
        // which a bare `RustType::Named` preserves, while the item's name is
        // canonicalized to `FloorHeating`. The usage walk must canonicalize the
        // reference the same way or it will miss the model and wrongly fall
        // back to deriving both serde traits.
        let module = Module {
            items: vec![
                strukt("Thing", vec![named_field("floor", "floor_heating")]),
                strukt("floor_heating", vec![]),
            ],
        };
        let service = Service {
            operations: vec![response_operation("Thing")],
            security_schemes: Vec::new(),
        };
        let targets = Targets {
            server: true,
            client: false,
        };
        let derives = model_derives(&module, &service, targets);
        let serde = derives
            .get("FloorHeating")
            .map(|derives| return derives.serde)
            .expect("model reached via canonicalized name");
        assert!(serde.serialize);
        assert!(
            !serde.deserialize,
            "a response-only model must skip Deserialize even when its schema name is not PascalCase"
        );
    }

    #[test]
    fn path_param_model_is_request_reachable() {
        let module = Module {
            items: vec![strukt("Status", vec![])],
        };
        let mut operation = response_operation("Ignored");
        operation.responses = vec![ResponseCase {
            variant: tname("NoContent"),
            status: ResponseStatus::Fixed(204),
            body: None,
            headers: Vec::new(),
            doc: None,
        }];
        operation.path_params = vec![Param {
            name: fname("status"),
            ty: RustType::Named("Status".to_owned()),
        }];
        let service = Service {
            operations: vec![operation],
            security_schemes: Vec::new(),
        };
        let targets = Targets {
            server: true,
            client: false,
        };
        let derives = model_derives(&module, &service, targets);
        let serde = derives
            .get("Status")
            .map(|derives| return derives.serde)
            .expect("path-param model reached");
        assert!(
            serde.deserialize,
            "the server's Path extractor deserializes a path-param model"
        );
        assert!(
            !serde.serialize,
            "a request-only model is never serialized on the server"
        );
    }

    #[test]
    fn response_header_model_is_response_reachable() {
        let module = Module {
            items: vec![strukt("Kind", vec![])],
        };
        let mut operation = response_operation("Ignored");
        operation.responses = vec![ResponseCase {
            variant: tname("Ok"),
            status: ResponseStatus::Fixed(200),
            body: None,
            headers: vec![ResponseHeader {
                name: fname("x_kind"),
                header_name: "X-Kind".to_owned(),
                ty: RustType::Named("Kind".to_owned()),
                required: true,
                doc: None,
            }],
            doc: None,
        }];
        let service = Service {
            operations: vec![operation],
            security_schemes: Vec::new(),
        };
        let targets = Targets {
            server: true,
            client: false,
        };
        let derives = model_derives(&module, &service, targets);
        let serde = derives
            .get("Kind")
            .map(|derives| return derives.serde)
            .expect("response-header model reached");
        assert!(
            serde.serialize,
            "a response-header model is reachable in the response direction"
        );
        assert!(!serde.deserialize);
    }
}
