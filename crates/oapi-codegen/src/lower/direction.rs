//! The request shape and the response shape that `readOnly` and `writeOnly`
//! describe.
//!
//! OpenAPI marks a property `readOnly` when a response carries it and a request
//! must not, and `writeOnly` for the opposite. The mark names a direction, not a
//! value, so one struct cannot state both.
//!
//! Each model therefore drops the properties that its direction does not carry.
//! A model that one direction reaches keeps its name. A model that both
//! directions reach becomes `<Name>Request` and `<Name>Response`, and so does
//! every model that both directions reach and that references it.
//!
//! `docs/design.md` states the rest of the reasoning.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;

use crate::emit::usage;
use crate::ir::Alias;
use crate::ir::Direction;
use crate::ir::Enum;
use crate::ir::EnumKind;
use crate::ir::Field;
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

fn project_validation(validation: &crate::ir::UnionValidation, direction: Direction) -> crate::ir::UnionValidation {
    let mut projected = validation.clone();
    for node in &mut projected.nodes {
        let mut removed = Vec::new();
        node.properties.retain(|(name, index)| {
            let Some(child) = validation.nodes.get(*index) else {
                return true;
            };
            let keywords = &child.keywords;
            let key = match direction {
                Direction::Request => "readOnly",
                Direction::Response => "writeOnly",
            };
            let keep = keywords.get(key).and_then(serde_json::Value::as_bool) != Some(true);
            if !keep {
                removed.push(name.clone());
            }
            return keep;
        });
        if let Some(required) = node
            .keywords
            .get_mut("required")
            .and_then(serde_json::Value::as_array_mut)
        {
            required.retain(|value| {
                return !value
                    .as_str()
                    .is_some_and(|name| return removed.iter().any(|removed| return removed == name));
            });
        }
    }
    return projected;
}

/// How one direction-sensitive model is projected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Projection {
    /// Both directions reach the model, so it becomes two items with suffixed
    /// names.
    Split,
    /// One direction reaches the model, so it keeps its name and drops only the
    /// properties that direction does not carry.
    Single(Direction),
}

/// Project every model a direction mark reaches onto the direction that carries
/// it, and point each API position at the shape it takes.
///
/// `service` is `None` for a models-only run, which has no operation to give a
/// direction. The pass splits every marked model there, because the run that
/// consumes those models does know the direction.
///
/// The pass leaves a document that marks no property untouched.
pub fn split_by_direction(module: &mut Module, service: Option<&mut Service>) {
    let projections = projections(module, service.as_deref());
    if projections.is_empty() {
        return;
    }
    let mut items = Vec::with_capacity(module.items.len() + projections.len());
    for item in module.items.drain(..) {
        match projections.get(item.name()) {
            Some(Projection::Split) => {
                items.push(project_item(&item, Direction::Request, &projections));
                items.push(project_item(&item, Direction::Response, &projections));
            }
            Some(Projection::Single(direction)) => items.push(project_item(&item, *direction, &projections)),
            None => items.push(item),
        }
    }
    module.items = items;
    if let Some(service) = service {
        project_service(service, &projections);
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

/// How each direction-sensitive model is projected, keyed by its logical name.
fn projections(module: &Module, service: Option<&Service>) -> BTreeMap<String, Projection> {
    let sensitive = sensitive_models(module);
    let Some(service) = service else {
        return sensitive
            .into_iter()
            .map(|name| return (name, Projection::Split))
            .collect();
    };

    let usage = usage::direction_usage(module, service);
    return sensitive
        .into_iter()
        .map(|name| {
            let used = usage.get(&name).copied().unwrap_or_default();
            let projection = match (used.request, used.response) {
                (true, false) => Projection::Single(Direction::Request),
                (false, true) => Projection::Single(Direction::Response),
                _ => Projection::Split,
            };
            return (name, projection);
        })
        .collect();
}

/// The models a direction mark reaches: the ones that mark a property, and
/// every model that references one of those.
///
/// The walk starts at a marked model and follows the reference graph in
/// reverse. The field type of a holder is what changes per direction.
fn sensitive_models(module: &Module) -> BTreeSet<String> {
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
    for (name, targets) in usage::adjacency(module) {
        for target in targets {
            referrers.entry(target).or_default().push(name.clone());
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

/// Build the shape of an item for one direction.
///
/// The name takes the suffix of the direction only when both directions reach
/// the model. The shape drops the properties that the other direction carries,
/// and points every reference to a split model at the shape of that model.
fn project_item(item: &Item, direction: Direction, projections: &BTreeMap<String, Projection>) -> Item {
    let split = matches!(projections.get(item.name()), Some(Projection::Split));
    let name_of = |name: &RustIdent| {
        if split {
            return projected_name(name.logical(), direction);
        }
        return name.clone();
    };
    return match item {
        Item::Struct(strukt) => {
            let fields: Vec<Field> = strukt
                .fields
                .iter()
                .filter(|field| return field.access.carried_by(direction))
                .map(|field| {
                    let mut projected = field.clone();
                    projected.ty = project_type(&field.ty, direction, projections);
                    return projected;
                })
                .collect();
            let dropped = fields.len() < strukt.fields.len();
            Item::Struct(Struct {
                name: name_of(&strukt.name),
                doc: projected_doc(&strukt.doc, strukt.name.logical(), direction, split, dropped),
                fields,
                additional_properties: strukt
                    .additional_properties
                    .as_ref()
                    .map(|ty| return project_type(ty, direction, projections)),
                ..strukt.clone()
            })
        }
        Item::Enum(enom) => Item::Enum(Enum {
            name: name_of(&enom.name),
            doc: projected_doc(&enom.doc, enom.name.logical(), direction, split, false),
            kind: match &enom.kind {
                EnumKind::Union(variants) | EnumKind::AnyOf(variants) => {
                    let variants = variants
                        .iter()
                        .map(|variant| {
                            return UnionVariant {
                                name: variant.name.clone(),
                                ty: project_type(&variant.ty, direction, projections),
                                validation: project_validation(&variant.validation, direction),
                            };
                        })
                        .collect();
                    if matches!(enom.kind, EnumKind::AnyOf(_)) {
                        EnumKind::AnyOf(variants)
                    } else {
                        EnumKind::Union(variants)
                    }
                }
                other => other.clone(),
            },
            ..enom.clone()
        }),
        Item::Alias(alias) => Item::Alias(Alias {
            name: name_of(&alias.name),
            doc: projected_doc(&alias.doc, alias.name.logical(), direction, split, false),
            ty: project_type(&alias.ty, direction, projections),
            ..alias.clone()
        }),
    };
}

/// Add a line about the direction, after the schema `description`.
///
/// A split shape always takes a line, because a reader meets two types where the
/// document declares one schema. The line names the keyword only when the
/// projection drops a property. A holder that splits because it references a
/// split model drops nothing, so its line states the direction alone.
///
/// A model that keeps its name takes a line only when the projection drops a
/// property. So a mark that costs a shape nothing leaves the generated file as
/// it was.
fn projected_doc(doc: &Option<String>, name: &str, direction: Direction, split: bool, dropped: bool) -> Option<String> {
    let note = match (split, dropped, direction) {
        (true, true, Direction::Request) => {
            format!("The request shape of `{name}`. A `readOnly` property is not part of it.")
        }
        (true, true, Direction::Response) => {
            format!("The response shape of `{name}`. A `writeOnly` property is not part of it.")
        }
        (true, false, Direction::Request) => format!("The request shape of `{name}`."),
        (true, false, Direction::Response) => format!("The response shape of `{name}`."),
        (false, false, _) => return doc.clone(),
        (false, true, Direction::Request) => {
            "Only a request carries this model, so a `readOnly` property is not part of it.".to_owned()
        }
        (false, true, Direction::Response) => {
            "Only a response carries this model, so a `writeOnly` property is not part of it.".to_owned()
        }
    };
    return Some(match doc {
        Some(text) => format!("{text}\n\n{note}"),
        None => note,
    });
}

/// Rewrite a type expression so a reference to a split model names the shape of
/// that model for `direction`. This leaves any other type as it is.
fn project_type(ty: &RustType, direction: Direction, projections: &BTreeMap<String, Projection>) -> RustType {
    return match ty {
        RustType::Named(name) => {
            // A `RustType::Named` holds the schema name the document wrote, so the
            // lookup canonicalizes it. Without that, a reference to `order-item`
            // would miss the `OrderItem` entry and keep the unsplit name.
            let canonical = to_ident(name, Case::Pascal);
            match projections.get(canonical.logical()) {
                Some(Projection::Split) => {
                    RustType::Named(projected_name(canonical.logical(), direction).logical().to_owned())
                }
                _ => ty.clone(),
            }
        }
        RustType::Vec(inner) => RustType::Vec(Box::new(project_type(inner, direction, projections))),
        RustType::Map(inner) => RustType::Map(Box::new(project_type(inner, direction, projections))),
        RustType::Option(inner) => RustType::Option(Box::new(project_type(inner, direction, projections))),
        RustType::Boxed(inner) => RustType::Boxed(Box::new(project_type(inner, direction, projections))),
        other => other.clone(),
    };
}

/// Point every API position at the shape of the direction it carries.
fn project_service(service: &mut Service, projections: &BTreeMap<String, Projection>) {
    for operation in &mut service.operations {
        for param in &mut operation.path_params {
            param.ty = project_type(&param.ty, Direction::Request, projections);
        }
        if let Some(query) = &mut operation.query {
            project_struct(query, Direction::Request, projections);
        }
        if let Some(headers) = &mut operation.headers {
            for param in &mut headers.params {
                param.ty = project_type(&param.ty, Direction::Request, projections);
            }
        }
        if let Some(cookies) = &mut operation.cookies {
            for param in &mut cookies.params {
                param.ty = project_type(&param.ty, Direction::Request, projections);
            }
        }
        if let Some(request) = &mut operation.request {
            match request {
                RequestPayload::Single(body) => body.ty = project_type(&body.ty, Direction::Request, projections),
                RequestPayload::Multipart(multipart) => {
                    for field in &mut multipart.fields {
                        field.ty = project_type(&field.ty, Direction::Request, projections);
                    }
                }
                RequestPayload::Negotiated(negotiated) => {
                    for variant in &mut negotiated.variants {
                        variant.body.ty = project_type(&variant.body.ty, Direction::Request, projections);
                    }
                }
            }
        }
        for case in &mut operation.responses {
            for header in &mut case.headers {
                header.ty = project_type(&header.ty, Direction::Response, projections);
            }
            match &mut case.body {
                Some(ResponseBody::Single(body)) => body.ty = project_type(&body.ty, Direction::Response, projections),
                Some(ResponseBody::Negotiated(negotiated)) => {
                    for variant in &mut negotiated.variants {
                        variant.body.ty = project_type(&variant.body.ty, Direction::Response, projections);
                    }
                }
                None => {}
            }
        }
    }
}

/// Rewrite the type of every field of a per-operation struct.
fn project_struct(strukt: &mut Struct, direction: Direction, projections: &BTreeMap<String, Projection>) {
    for field in &mut strukt.fields {
        field.ty = project_type(&field.ty, direction, projections);
    }
    if let Some(additional) = &mut strukt.additional_properties {
        *additional = project_type(additional, direction, projections);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::ir::Access;
    use crate::ir::Field;
    use crate::loader::Spec;

    /// Lower the schemas of an inline document and split them, the way a
    /// models-only run does.
    fn split_yaml(yaml: &str) -> Module {
        let doc: openapiv3::OpenAPI = serde_yaml::from_str(yaml).expect("parse spec");
        let spec = Spec::from_parts(doc, PathBuf::from("inline.yaml"));
        let names = crate::lower::rename::type_renames(&spec, None).expect("resolve names");
        let mut module = crate::lower::generate_models(&spec, &names).expect("lower schemas");
        split_by_direction(&mut module, None);
        return module;
    }

    /// Lower an inline document with its operations and split them, the way a
    /// server or client run does.
    fn split_service_yaml(yaml: &str) -> Module {
        let doc: openapiv3::OpenAPI = serde_yaml::from_str(yaml).expect("parse spec");
        let spec = Spec::from_parts(doc, PathBuf::from("inline.yaml"));
        let names = crate::lower::rename::type_renames(&spec, None).expect("resolve names");
        let mut module = crate::lower::generate_models(&spec, &names).expect("lower schemas");
        let mut service =
            crate::lower::generate_service(&spec, &Default::default(), "Response").expect("lower operations");
        crate::lower::rewrite_service(&mut service, names.renames());
        split_by_direction(&mut module, Some(&mut service));
        return module;
    }

    /// The doc text of the item called `name`.
    fn doc(module: &Module, name: &str) -> Option<String> {
        for item in &module.items {
            if item.name() == name {
                return match item {
                    Item::Struct(strukt) => strukt.doc.clone(),
                    Item::Enum(enom) => enom.doc.clone(),
                    Item::Alias(alias) => alias.doc.clone(),
                };
            }
        }
        panic!("no item named `{name}`");
    }

    /// A document whose only operation returns `Account`, so no request reaches
    /// it.
    fn response_only(schemas: &str) -> String {
        return format!(
            "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths:\n  /accounts:\n    get:\n      operationId: listAccounts\n      responses:\n        '200':\n          description: ok\n          content:\n            application/json:\n              schema:\n                $ref: '#/components/schemas/Account'\ncomponents:\n  schemas:\n{schemas}"
        );
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
    fn one_direction_keeps_the_name_and_drops_nothing_it_carries() {
        let module = split_service_yaml(&response_only(
            "    Account:\n      description: An account.\n      type: object\n      properties:\n        id:\n          type: string\n          readOnly: true\n        email:\n          type: string\n",
        ));
        assert_eq!(names(&module), vec!["Account"]);
        assert_eq!(fields(&module, "Account"), vec!["id", "email"]);
        assert_eq!(doc(&module, "Account"), Some("An account.".to_owned()));
    }

    #[test]
    fn one_direction_keeps_the_name_and_drops_what_it_cannot_carry() {
        let module = split_service_yaml(&response_only(
            "    Account:\n      type: object\n      properties:\n        email:\n          type: string\n        secret:\n          type: string\n          writeOnly: true\n",
        ));
        assert_eq!(names(&module), vec!["Account"]);
        assert_eq!(fields(&module, "Account"), vec!["email"]);
        assert_eq!(
            doc(&module, "Account"),
            Some("Only a response carries this model, so a `writeOnly` property is not part of it.".to_owned())
        );
    }

    /// A split shape names the keyword only when it drops a property.
    ///
    /// A holder splits because it references a split model. It drops nothing, so
    /// a line about a `readOnly` property would state a fault that is not there.
    #[test]
    fn a_split_shape_that_drops_nothing_states_the_direction_alone() {
        let module = split_yaml(&format!(
            "{PREAMBLE}    Account:\n      type: object\n      properties:\n        id:\n          type: string\n          readOnly: true\n    Envelope:\n      type: object\n      properties:\n        account:\n          $ref: '#/components/schemas/Account'\n"
        ));
        assert_eq!(
            doc(&module, "AccountRequest"),
            Some("The request shape of `Account`. A `readOnly` property is not part of it.".to_owned())
        );
        assert_eq!(
            doc(&module, "AccountResponse"),
            Some("The response shape of `Account`.".to_owned()),
            "the response drops nothing, because no property is `writeOnly`",
        );
        assert_eq!(
            doc(&module, "EnvelopeRequest"),
            Some("The request shape of `Envelope`.".to_owned())
        );
        assert_eq!(
            doc(&module, "EnvelopeResponse"),
            Some("The response shape of `Envelope`.".to_owned())
        );
    }

    #[test]
    fn a_holder_that_one_direction_reaches_keeps_its_name_and_names_the_split_shape() {
        let yaml = "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths:\n  /accounts:\n    post:\n      operationId: createAccount\n      requestBody:\n        content:\n          application/json:\n            schema:\n              $ref: '#/components/schemas/Account'\n      responses:\n        '200':\n          description: ok\n          content:\n            application/json:\n              schema:\n                $ref: '#/components/schemas/Page'\ncomponents:\n  schemas:\n    Account:\n      type: object\n      properties:\n        id:\n          type: string\n          readOnly: true\n        email:\n          type: string\n    Page:\n      type: object\n      properties:\n        items:\n          type: array\n          items:\n            $ref: '#/components/schemas/Account'\n";
        let module = split_service_yaml(yaml);
        assert_eq!(names(&module), vec!["AccountRequest", "AccountResponse", "Page"]);
        let Some(Item::Struct(page)) = module.items.iter().find(|item| return item.name() == "Page") else {
            panic!("no struct named `Page`");
        };
        assert_eq!(page.fields[0].ty.label(), "Option<Vec<AccountResponse>>");
    }

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
