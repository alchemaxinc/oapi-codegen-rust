//! Splitting a model into the request shape and the response shape that its
//! `readOnly` and `writeOnly` properties describe.
//!
//! OpenAPI marks a property `readOnly` when a response may carry it and a
//! request must not, and `writeOnly` for the opposite. The mark names a
//! direction, not a value, so one struct cannot state both: the same `Order`
//! type reaches a request body and a response body, and a serde attribute that
//! is right for one is wrong for the other. A server deserializes a request and
//! serializes a response, and a client does the reverse, so an attribute cannot
//! even be chosen per target.
//!
//! So a marked model becomes two models. `Order` with a `readOnly` `id` emits
//! `OrderRequest`, which has no `id`, and `OrderResponse`, which has one. Each
//! API position then names the shape its direction carries: a request body, a
//! parameter and a multipart part take the request shape, and a response body
//! and a response header take the response shape.
//!
//! The split spreads along model references. A model holding a marked model
//! cannot keep one name either, because its field type differs per direction,
//! so `Envelope { order: Order }` becomes `EnvelopeRequest { order: OrderRequest }`
//! and `EnvelopeResponse { order: OrderResponse }`. A model that no mark reaches
//! keeps its name, so a document that uses neither keyword generates exactly
//! what it generated before.
//!
//! The split reads the marks only. It does not read how the operations use a
//! model, so the two names a schema takes stay the same when an operation is
//! added or removed. A shape that no operation reaches is then dropped by
//! [`crate::lower::prune`], the same way any unused model is.

use std::collections::BTreeSet;
use std::collections::HashMap;

use crate::ir::Alias;
use crate::ir::Direction;
use crate::ir::Enum;
use crate::ir::EnumKind;
use crate::ir::Item;
use crate::ir::Module;
use crate::ir::RequestPayload;
use crate::ir::ResponseBody;
use crate::ir::RustType;
use crate::ir::Service;
use crate::ir::Struct;
use crate::ir::UnionVariant;
use crate::naming::Case;
use crate::naming::RustIdent;
use crate::naming::to_ident;

/// The suffix the request shape of a split model takes.
pub const REQUEST_SUFFIX: &str = "Request";
/// The suffix the response shape of a split model takes.
pub const RESPONSE_SUFFIX: &str = "Response";

/// Replace every model a direction mark reaches with its request shape and its
/// response shape, and point each API position at the shape its direction
/// carries.
///
/// `service` is `None` for a models-only run, which has no operation to give a
/// direction. Both shapes are emitted there, because a models crate is consumed
/// by a run that does know the direction.
///
/// A document that marks no property leaves both the module and the service
/// untouched.
pub fn split_by_direction(module: &mut Module, service: Option<&mut Service>) {
    let split = split_models(module);
    if split.is_empty() {
        return;
    }
    let mut items = Vec::with_capacity(module.items.len() + split.len());
    for item in module.items.drain(..) {
        if split.contains(item.name()) {
            items.push(project_item(&item, Direction::Request, &split));
            items.push(project_item(&item, Direction::Response, &split));
        } else {
            items.push(item);
        }
    }
    module.items = items;
    if let Some(service) = service {
        project_service(service, &split);
    }
}

/// The name a model takes in one direction.
pub fn projected_name(name: &str, direction: Direction) -> RustIdent {
    let suffix = match direction {
        Direction::Request => REQUEST_SUFFIX,
        Direction::Response => RESPONSE_SUFFIX,
    };
    return to_ident(&format!("{name} {suffix}"), Case::Pascal);
}

/// The models that must be split: the ones marking a property, plus every model
/// that reaches one of those through a field, a variant, or an alias target.
///
/// The walk runs up the reference graph, from a marked model to the models
/// naming it, because a holder's field type is what changes per direction.
fn split_models(module: &Module) -> BTreeSet<String> {
    let mut marked: BTreeSet<String> = module
        .items
        .iter()
        .filter(|item| return marks_a_direction(item))
        .map(|item| return item.name().to_owned())
        .collect();
    if marked.is_empty() {
        return marked;
    }

    let mut referrers: HashMap<String, Vec<String>> = HashMap::new();
    for item in &module.items {
        for target in item_references(item) {
            referrers.entry(target).or_default().push(item.name().to_owned());
        }
    }

    let mut stack: Vec<String> = marked.iter().cloned().collect();
    while let Some(name) = stack.pop() {
        let Some(parents) = referrers.get(&name) else {
            continue;
        };
        for parent in parents {
            if marked.insert(parent.clone()) {
                stack.push(parent.clone());
            }
        }
    }
    return marked;
}

/// Whether an item declares a property that only one direction carries.
fn marks_a_direction(item: &Item) -> bool {
    let Item::Struct(strukt) = item else {
        return false;
    };
    return strukt.fields.iter().any(|field| {
        return field.access != crate::ir::Access::ReadWrite;
    });
}

/// The names of the generated models an item references, canonicalized the way
/// [`Item::name`] spells them.
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
            if let EnumKind::Union(variants) = &enom.kind {
                for variant in variants {
                    collect_named(&variant.ty, &mut names);
                }
            }
        }
        Item::Alias(alias) => collect_named(&alias.ty, &mut names),
    }
    return names;
}

/// Collect the model a type expression names, looking through the wrappers.
///
/// A [`RustType::Named`] still holds the schema name the document wrote, so it
/// is run through [`to_ident`] to match the item names the graph is keyed by.
fn collect_named(ty: &RustType, out: &mut Vec<String>) {
    match ty {
        RustType::Named(name) => out.push(to_ident(name, Case::Pascal).logical().to_owned()),
        RustType::Vec(inner) | RustType::Map(inner) | RustType::Option(inner) | RustType::Boxed(inner) => {
            collect_named(inner, out);
        }
        _ => {}
    }
}

/// Build one direction's shape of an item: its name takes the direction's
/// suffix, the properties the other direction carries are dropped, and every
/// reference to a split model points at that model's shape.
fn project_item(item: &Item, direction: Direction, split: &BTreeSet<String>) -> Item {
    return match item {
        Item::Struct(strukt) => Item::Struct(Struct {
            name: projected_name(strukt.name.logical(), direction),
            doc: projected_doc(&strukt.doc, strukt.name.logical(), direction),
            fields: strukt
                .fields
                .iter()
                .filter(|field| return field.access.carried_by(direction))
                .map(|field| {
                    let mut projected = field.clone();
                    projected.ty = project_type(&field.ty, direction, split);
                    return projected;
                })
                .collect(),
            additional_properties: strukt
                .additional_properties
                .as_ref()
                .map(|ty| return project_type(ty, direction, split)),
            ..strukt.clone()
        }),
        Item::Enum(enom) => Item::Enum(Enum {
            name: projected_name(enom.name.logical(), direction),
            doc: projected_doc(&enom.doc, enom.name.logical(), direction),
            kind: match &enom.kind {
                EnumKind::Union(variants) => EnumKind::Union(
                    variants
                        .iter()
                        .map(|variant| {
                            return UnionVariant {
                                name: variant.name.clone(),
                                ty: project_type(&variant.ty, direction, split),
                            };
                        })
                        .collect(),
                ),
                other => other.clone(),
            },
            ..enom.clone()
        }),
        Item::Alias(alias) => Item::Alias(Alias {
            name: projected_name(alias.name.logical(), direction),
            doc: projected_doc(&alias.doc, alias.name.logical(), direction),
            ty: project_type(&alias.ty, direction, split),
            ..alias.clone()
        }),
    };
}

/// Add a line naming the direction a shape carries, after whatever the schema
/// `description` already says.
///
/// A reader meets two types where the document declares one schema, so the
/// generated file states which of the two this is and where the name came from.
fn projected_doc(doc: &Option<String>, name: &str, direction: Direction) -> Option<String> {
    let note = match direction {
        Direction::Request => format!("The request shape of `{name}`. A `readOnly` property is not part of it."),
        Direction::Response => format!("The response shape of `{name}`. A `writeOnly` property is not part of it."),
    };
    return Some(match doc {
        Some(text) => format!("{text}\n\n{note}"),
        None => note,
    });
}

/// Rewrite a type expression so a reference to a split model names that model's
/// shape for `direction`. Anything else is left as it is.
fn project_type(ty: &RustType, direction: Direction, split: &BTreeSet<String>) -> RustType {
    return match ty {
        RustType::Named(name) => {
            let canonical = to_ident(name, Case::Pascal);
            if split.contains(canonical.logical()) {
                RustType::Named(projected_name(canonical.logical(), direction).logical().to_owned())
            } else {
                ty.clone()
            }
        }
        RustType::Vec(inner) => RustType::Vec(Box::new(project_type(inner, direction, split))),
        RustType::Map(inner) => RustType::Map(Box::new(project_type(inner, direction, split))),
        RustType::Option(inner) => RustType::Option(Box::new(project_type(inner, direction, split))),
        RustType::Boxed(inner) => RustType::Boxed(Box::new(project_type(inner, direction, split))),
        other => other.clone(),
    };
}

/// Point every API position at the shape of the direction it carries, and drop
/// the multipart parts a request must not send.
fn project_service(service: &mut Service, split: &BTreeSet<String>) {
    for operation in &mut service.operations {
        for param in &mut operation.path_params {
            param.ty = project_type(&param.ty, Direction::Request, split);
        }
        if let Some(query) = &mut operation.query {
            project_struct(query, Direction::Request, split);
        }
        if let Some(headers) = &mut operation.headers {
            for param in &mut headers.params {
                param.ty = project_type(&param.ty, Direction::Request, split);
            }
        }
        if let Some(cookies) = &mut operation.cookies {
            for param in &mut cookies.params {
                param.ty = project_type(&param.ty, Direction::Request, split);
            }
        }
        if let Some(request) = &mut operation.request {
            match request {
                RequestPayload::Single(body) => body.ty = project_type(&body.ty, Direction::Request, split),
                RequestPayload::Multipart(multipart) => {
                    for field in &mut multipart.fields {
                        field.ty = project_type(&field.ty, Direction::Request, split);
                    }
                }
                RequestPayload::Negotiated(negotiated) => {
                    for variant in &mut negotiated.variants {
                        variant.body.ty = project_type(&variant.body.ty, Direction::Request, split);
                    }
                }
            }
        }
        for case in &mut operation.responses {
            for header in &mut case.headers {
                header.ty = project_type(&header.ty, Direction::Response, split);
            }
            match &mut case.body {
                Some(ResponseBody::Single(body)) => body.ty = project_type(&body.ty, Direction::Response, split),
                Some(ResponseBody::Negotiated(negotiated)) => {
                    for variant in &mut negotiated.variants {
                        variant.body.ty = project_type(&variant.body.ty, Direction::Response, split);
                    }
                }
                None => {}
            }
        }
    }
}

/// Rewrite the type of every field of a per-operation struct.
fn project_struct(strukt: &mut Struct, direction: Direction, split: &BTreeSet<String>) {
    for field in &mut strukt.fields {
        field.ty = project_type(&field.ty, direction, split);
    }
    if let Some(additional) = &mut strukt.additional_properties {
        *additional = project_type(additional, direction, split);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::ir::Access;
    use crate::ir::Field;
    use crate::loader::Spec;

    /// Lower an inline document's schemas and split them, which is the pipeline
    /// a models-only run does.
    fn split_yaml(yaml: &str) -> Module {
        let doc: openapiv3::OpenAPI = serde_yaml::from_str(yaml).expect("parse spec");
        let spec = Spec::from_parts(doc, PathBuf::from("inline.yaml"));
        let names = crate::lower::rename::type_renames(&spec, None).expect("resolve names");
        let mut module = crate::lower::generate_models(&spec, &names).expect("lower schemas");
        split_by_direction(&mut module, None);
        return module;
    }

    /// The item names a module declares, in emission order.
    fn names(module: &Module) -> Vec<String> {
        return module.items.iter().map(|item| return item.name().to_owned()).collect();
    }

    /// The field names of the struct called `name`.
    fn fields(module: &Module, name: &str) -> Vec<String> {
        for item in &module.items {
            if let Item::Struct(strukt) = item
                && strukt.name.logical() == name
            {
                return strukt
                    .fields
                    .iter()
                    .map(|field| return field.name.logical().to_owned())
                    .collect();
            }
        }
        panic!("no struct named `{name}`");
    }

    const PREAMBLE: &str = "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\ncomponents:\n  schemas:\n";

    #[test]
    fn a_document_with_no_mark_is_left_alone() {
        let module = split_yaml(&format!(
            "{PREAMBLE}    Widget:\n      type: object\n      properties:\n        name:\n          type: string\n"
        ));
        assert_eq!(names(&module), vec!["Widget"]);
    }

    #[test]
    fn a_marked_property_leaves_the_shape_the_other_direction_carries() {
        let module = split_yaml(&format!(
            "{PREAMBLE}    Account:\n      type: object\n      properties:\n        id:\n          type: string\n          readOnly: true\n        secret:\n          type: string\n          writeOnly: true\n        email:\n          type: string\n"
        ));
        assert_eq!(names(&module), vec!["AccountRequest", "AccountResponse"]);
        assert_eq!(fields(&module, "AccountRequest"), vec!["secret", "email"]);
        assert_eq!(fields(&module, "AccountResponse"), vec!["id", "email"]);
    }

    #[test]
    fn a_mark_on_a_referenced_schema_reaches_the_property_that_names_it() {
        let module = split_yaml(&format!(
            "{PREAMBLE}    Id:\n      type: string\n      readOnly: true\n    Account:\n      type: object\n      properties:\n        id:\n          $ref: '#/components/schemas/Id'\n        email:\n          type: string\n"
        ));
        assert_eq!(fields(&module, "AccountRequest"), vec!["email"]);
        assert_eq!(fields(&module, "AccountResponse"), vec!["id", "email"]);
    }

    #[test]
    fn an_all_of_member_carries_its_marks_into_the_merged_shape() {
        let module = split_yaml(&format!(
            "{PREAMBLE}    Timestamps:\n      type: object\n      properties:\n        createdAt:\n          type: string\n          readOnly: true\n    Account:\n      allOf:\n        - $ref: '#/components/schemas/Timestamps'\n        - type: object\n          properties:\n            email:\n              type: string\n"
        ));
        assert_eq!(fields(&module, "AccountRequest"), vec!["email"]);
        assert_eq!(fields(&module, "AccountResponse"), vec!["created_at", "email"]);
    }

    #[test]
    fn a_holder_of_a_split_model_splits_and_names_the_matching_shape() {
        let module = split_yaml(&format!(
            "{PREAMBLE}    Account:\n      type: object\n      properties:\n        id:\n          type: string\n          readOnly: true\n    Envelope:\n      type: object\n      properties:\n        account:\n          $ref: '#/components/schemas/Account'\n"
        ));
        assert_eq!(
            names(&module),
            vec![
                "AccountRequest",
                "AccountResponse",
                "EnvelopeRequest",
                "EnvelopeResponse"
            ]
        );
        let request = module
            .items
            .iter()
            .find(|item| return item.name() == "EnvelopeRequest")
            .expect("request shape");
        let Item::Struct(strukt) = request else {
            panic!("expected a struct");
        };
        assert_eq!(
            strukt.fields.first().map(|field| return field.ty.label()),
            Some("Option<AccountRequest>".to_owned())
        );
    }

    #[test]
    fn both_marks_on_one_property_are_rejected() {
        let doc: openapiv3::OpenAPI = serde_yaml::from_str(&format!(
            "{PREAMBLE}    Account:\n      type: object\n      properties:\n        secret:\n          type: string\n          readOnly: true\n          writeOnly: true\n"
        ))
        .expect("parse spec");
        let spec = Spec::from_parts(doc, PathBuf::from("inline.yaml"));
        let names = crate::lower::rename::type_renames(&spec, None).expect("resolve names");
        let error = crate::lower::generate_models(&spec, &names).expect_err("both marks must be rejected");
        assert!(
            format!("{error}").contains("`readOnly` and `writeOnly` are both set"),
            "unexpected message: {error}"
        );
    }

    #[test]
    fn a_name_that_is_not_pascal_case_still_matches_the_split_set() {
        // A `RustType::Named` holds the schema name the document wrote, so the
        // lookup canonicalizes it. Without that, a reference to `order-item`
        // would miss the `OrderItem` entry and keep the unsplit name.
        let module = split_yaml(&format!(
            "{PREAMBLE}    order-item:\n      type: object\n      properties:\n        id:\n          type: string\n          readOnly: true\n    Basket:\n      type: object\n      properties:\n        item:\n          $ref: '#/components/schemas/order-item'\n"
        ));
        assert!(names(&module).contains(&"OrderItemRequest".to_owned()));
        let Some(Item::Struct(basket)) = module.items.iter().find(|item| return item.name() == "BasketRequest") else {
            panic!("no request shape for `Basket`");
        };
        assert_eq!(
            basket.fields.first().map(|field| return field.ty.label()),
            Some("Option<OrderItemRequest>".to_owned())
        );
    }

    #[test]
    fn access_reports_the_direction_that_carries_the_property() {
        let cases = [
            (Access::ReadWrite, true, true),
            (Access::ReadOnly, false, true),
            (Access::WriteOnly, true, false),
        ];
        for (access, request, response) in cases {
            assert_eq!(access.carried_by(Direction::Request), request, "{access:?} request");
            assert_eq!(access.carried_by(Direction::Response), response, "{access:?} response");
        }
    }

    #[test]
    fn a_field_with_no_mark_keeps_the_default_access() {
        let field = Field {
            name: to_ident("name", Case::Snake),
            rename: None,
            doc: None,
            deprecated: None,
            ty: RustType::String,
            required: true,
            omit_empty: None,
            serde_skip: false,
            default: None,
            constraints: None,
            access: Access::default(),
        };
        assert_eq!(field.access, Access::ReadWrite);
    }
}
