//! Deciding each generated model's serde derive set from how the API uses it.
//!
//! A generated type only needs a serde trait in the direction its API position
//! exercises: a server serializes response bodies and deserializes request
//! bodies; a client does the reverse. Deriving a trait a type never needs would
//! impose an unsatisfiable bound on a reused `x-rust-type` target — e.g. forcing
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
use crate::emit::models::SerdeDerives;
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
struct Usage {
    /// Reachable from a request body or request-input struct.
    request: bool,
    /// Reachable from a response body.
    response: bool,
}

/// Compute the serde derive set for every generated model, keyed by its logical
/// name. Names absent from the returned map are unreferenced by any operation
/// (for example under `skip-prune`) and should derive both traits.
pub(crate) fn model_derives(module: &Module, service: &Service, targets: Targets) -> HashMap<String, SerdeDerives> {
    let adjacency = adjacency(module);
    let mut usage: HashMap<String, Usage> = HashMap::new();

    for name in request_seeds(service) {
        mark(&adjacency, &name, &mut usage, Direction::Request);
    }
    for name in response_seeds(service) {
        mark(&adjacency, &name, &mut usage, Direction::Response);
    }

    return usage
        .into_iter()
        .map(|(name, used)| {
            let serde = SerdeDerives {
                serialize: (targets.server && used.response) || (targets.client && used.request),
                deserialize: (targets.server && used.request) || (targets.client && used.response),
            };
            return (name, serde);
        })
        .collect();
}

/// The direction a seed propagates.
#[derive(Debug, Clone, Copy)]
enum Direction {
    Request,
    Response,
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
fn adjacency(module: &Module) -> HashMap<String, Vec<String>> {
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
            if let crate::ir::EnumKind::Union(variants) = &enom.kind {
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
/// non-PascalCase schema name would fail the usage lookup and fall back to
/// deriving both serde traits. External and verbatim types name no generated
/// model, so they contribute nothing.
fn collect_named(ty: &RustType, out: &mut Vec<String>) {
    match ty {
        RustType::Named(name) => out.push(to_ident(name, Case::Pascal).logical().to_owned()),
        RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) => collect_named(inner, out),
        _ => {}
    }
}

/// Model names seeded as request-reachable: request payloads plus the fields of
/// each operation's query-input struct.
fn request_seeds(service: &Service) -> Vec<String> {
    let mut names = Vec::new();
    for operation in &service.operations {
        if let Some(query) = &operation.query {
            struct_field_names(query, &mut names);
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

/// Model names seeded as response-reachable: every response variant's body.
fn response_seeds(service: &Service) -> Vec<String> {
    let mut names = Vec::new();
    for operation in &service.operations {
        for case in &operation.responses {
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
    use crate::ir::Alias;
    use crate::ir::Body;
    use crate::ir::Field;
    use crate::ir::Operation;
    use crate::ir::ResponseCase;
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
        };
    }

    fn strukt(name: &str, fields: Vec<Field>) -> Item {
        return Item::Struct(Struct {
            name: tname(name),
            doc: None,
            deprecated: None,
            fields,
            additional_properties: None,
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
            let serde = derives.get(name).copied().expect("model reached");
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
        let serde = derives.get("Thing").copied().expect("model reached");
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
                    ty: RustType::Verbatim("crate::domain::FloorHeating".to_owned()),
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
        let serde = derives.get("FloorHeating").copied().expect("alias reached");
        assert!(serde.serialize);
        assert!(!serde.deserialize);
    }

    #[test]
    fn non_pascal_schema_name_is_matched_after_canonicalization() {
        // The referencing field carries the raw schema name (`floor_heating`),
        // which a bare `RustType::Named` preserves, while the item's name is
        // canonicalized to `FloorHeating`. The usage walk must canonicalize the
        // reference the same way or it would miss the model and wrongly fall
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
            .copied()
            .expect("model reached via canonicalized name");
        assert!(serde.serialize);
        assert!(
            !serde.deserialize,
            "a response-only model must skip Deserialize even when its schema name is not PascalCase"
        );
    }
}
