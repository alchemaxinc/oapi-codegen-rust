use openapiv3::AdditionalProperties;
use openapiv3::ObjectType;
use openapiv3::ReferenceOr;
use openapiv3::Schema;
use openapiv3::SchemaKind;
use openapiv3::Type;

use crate::error::Error;
use crate::error::Result;
use crate::loader::Spec;
use crate::lower::schema::MAX_SCHEMA_DEPTH;

fn unsupported(path: &str, reason: &str) -> Error {
    return Error::UnsupportedSchema {
        path: path.to_owned(),
        reason: format!("allOf intersection: {reason}"),
    };
}

pub(super) fn merge(spec: &Spec, path: &str, members: &[ReferenceOr<Schema>]) -> Result<ObjectType> {
    let mut objects = Vec::new();
    collect(spec, path, members, &mut objects, 0)?;
    let mut merged = ObjectType::default();
    for object in &objects {
        if matches!(object.additional_properties, Some(AdditionalProperties::Any(true))) {
            merged.additional_properties = Some(AdditionalProperties::Any(true));
        }
        for (name, property) in &object.properties {
            let property = match merged.properties.get(name) {
                Some(previous) => intersect(spec, &format!("{path}.{name}"), previous, property)?,
                None => property.clone(),
            };
            merged.properties.insert(name.clone(), property);
        }
        for name in &object.required {
            if !merged.required.contains(name) {
                merged.required.push(name.clone());
            }
        }
    }
    for object in &objects {
        if matches!(object.additional_properties, Some(AdditionalProperties::Any(false))) {
            if merged
                .properties
                .keys()
                .any(|name| return !object.properties.contains_key(name))
                || merged
                    .required
                    .iter()
                    .any(|name| return !object.properties.contains_key(name))
            {
                return Err(unsupported(
                    path,
                    "a closed member forbids a property from another member",
                ));
            }
            merged.additional_properties = Some(AdditionalProperties::Any(false));
        }
    }
    if merged
        .required
        .iter()
        .any(|name| return !merged.properties.contains_key(name))
    {
        return Err(unsupported(path, "a required property has no declared schema"));
    }
    return Ok(merged);
}

fn collect(
    spec: &Spec,
    path: &str,
    members: &[ReferenceOr<Schema>],
    objects: &mut Vec<ObjectType>,
    depth: usize,
) -> Result<()> {
    if depth >= MAX_SCHEMA_DEPTH {
        return Err(Error::SchemaDepthExceeded {
            path: path.to_owned(),
            limit: MAX_SCHEMA_DEPTH,
        });
    }
    if members.is_empty() {
        return Err(unsupported(path, "an empty composition is not supported"));
    }
    for member in members {
        let schema = match member {
            ReferenceOr::Item(schema) => schema,
            ReferenceOr::Reference { reference } => spec.resolve(reference)?,
        };
        let data = &schema.schema_data;
        if data.nullable
            || data.read_only
            || data.write_only
            || data.deprecated
            || data.default.is_some()
            || !data.extensions.is_empty()
            || data.discriminator.is_some()
        {
            return Err(unsupported(
                path,
                "member nullability, access, deprecation, defaults, extensions, or discriminators cannot be flattened",
            ));
        }
        match &schema.schema_kind {
            SchemaKind::Type(Type::Object(object)) => {
                if object.min_properties.is_some() || object.max_properties.is_some() {
                    return Err(unsupported(
                        path,
                        "object property-count constraints cannot be flattened",
                    ));
                }
                if matches!(object.additional_properties, Some(AdditionalProperties::Schema(_))) {
                    return Err(unsupported(
                        path,
                        "schema-valued additionalProperties cannot be flattened",
                    ));
                }
                objects.push(object.clone());
            }
            SchemaKind::AllOf { all_of } => collect(spec, path, all_of, objects, depth + 1)?,
            _ => return Err(unsupported(path, "members must be objects or references to objects")),
        }
    }
    return Ok(());
}

fn resolve(spec: &Spec, path: &str, property: &ReferenceOr<Box<Schema>>) -> Result<Schema> {
    let mut schema = match property {
        ReferenceOr::Item(schema) => *schema.clone(),
        ReferenceOr::Reference { reference } => spec.resolve(reference)?.clone(),
    };
    for _ in 0..MAX_SCHEMA_DEPTH {
        let SchemaKind::AllOf { all_of } = &schema.schema_kind else {
            return Ok(schema);
        };
        let [member] = all_of.as_slice() else {
            return Err(unsupported(path, "overlapping composed properties are not supported"));
        };
        let mut target = match member {
            ReferenceOr::Item(schema) => schema.clone(),
            ReferenceOr::Reference { reference } => spec.resolve(reference)?.clone(),
        };
        let mut data = schema.schema_data.clone();
        data.nullable = false;
        if data != openapiv3::SchemaData::default() {
            return Err(unsupported(
                path,
                "overlapping reference wrappers carry unsupported metadata",
            ));
        }
        target.schema_data.nullable |= schema.schema_data.nullable;
        schema = target;
    }
    return Err(Error::SchemaDepthExceeded {
        path: path.to_owned(),
        limit: MAX_SCHEMA_DEPTH,
    });
}

fn intersect(
    spec: &Spec,
    path: &str,
    left: &ReferenceOr<Box<Schema>>,
    right: &ReferenceOr<Box<Schema>>,
) -> Result<ReferenceOr<Box<Schema>>> {
    if left == right && matches!(left, ReferenceOr::Reference { .. }) {
        return Ok(left.clone());
    }
    let mut left = resolve(spec, path, left)?;
    let mut right = resolve(spec, path, right)?;
    let nullable = left.schema_data.nullable && right.schema_data.nullable;
    left.schema_data.nullable = nullable;
    right.schema_data.nullable = nullable;
    if left.schema_data != right.schema_data {
        return Err(unsupported(
            path,
            "overlapping properties have different metadata or extensions",
        ));
    }
    if left.schema_data.extensions.contains_key("x-rust-type") && left.schema_kind != right.schema_kind {
        return Err(unsupported(path, "custom-type property constraints differ"));
    }
    if left.schema_kind != right.schema_kind
        && ["x-enum-varnames", "x-enumNames"]
            .iter()
            .any(|key| return left.schema_data.extensions.contains_key(*key))
    {
        return Err(unsupported(
            path,
            "enum intersections with positional variant names are not supported",
        ));
    }
    match (&mut left.schema_kind, &right.schema_kind) {
        (SchemaKind::Type(Type::String(a)), SchemaKind::Type(Type::String(b))) => {
            combine_keyword(path, "formats", &mut a.format, &b.format)?;
            combine_keyword(path, "patterns", &mut a.pattern, &b.pattern)?;
            a.min_length = tighter(a.min_length, b.min_length, true);
            a.max_length = tighter(a.max_length, b.max_length, false);
            check_range(path, a.min_length, a.max_length)?;
            if nullable
                && !matches!(
                    super::schema::string_format_type(&a.format),
                    crate::ir::RustType::String
                )
                && (a.pattern.is_some() || a.min_length.is_some() || a.max_length.is_some())
            {
                return Err(unsupported(
                    path,
                    "nullable formatted-string constraints cannot be enforced",
                ));
            }
            narrow_enum(path, &mut a.enumeration, &b.enumeration)?;
            if !a.enumeration.is_empty()
                && (nullable || a.pattern.is_some() || a.min_length.is_some() || a.max_length.is_some())
            {
                return Err(unsupported(
                    path,
                    "string enum intersections with nullability or string constraints are not supported",
                ));
            }
        }
        (SchemaKind::Type(Type::Integer(a)), SchemaKind::Type(Type::Integer(b))) => {
            combine_keyword(path, "formats", &mut a.format, &b.format)?;
            combine_keyword(path, "multipleOf", &mut a.multiple_of, &b.multiple_of)?;
            (a.minimum, a.exclusive_minimum) =
                bound(a.minimum, a.exclusive_minimum, b.minimum, b.exclusive_minimum, true);
            (a.maximum, a.exclusive_maximum) =
                bound(a.maximum, a.exclusive_maximum, b.maximum, b.exclusive_maximum, false);
            check_range(path, a.minimum, a.maximum)?;
            narrow_enum(path, &mut a.enumeration, &b.enumeration)?;
            if !a.enumeration.is_empty()
                && (nullable || a.minimum.is_some() || a.maximum.is_some() || a.multiple_of.is_some())
            {
                return Err(unsupported(
                    path,
                    "integer enum intersections with nullability or numeric constraints are not supported",
                ));
            }
        }
        (SchemaKind::Type(Type::Number(a)), SchemaKind::Type(Type::Number(b))) => {
            if [a.minimum, a.maximum, b.minimum, b.maximum]
                .into_iter()
                .flatten()
                .any(|value| return !value.is_finite())
            {
                return Err(unsupported(path, "numeric bounds must be finite"));
            }
            combine_keyword(path, "formats", &mut a.format, &b.format)?;
            combine_keyword(path, "multipleOf", &mut a.multiple_of, &b.multiple_of)?;
            if !a.enumeration.is_empty() || !b.enumeration.is_empty() {
                return Err(unsupported(path, "number enums are not supported in this overlap"));
            }
            (a.minimum, a.exclusive_minimum) =
                bound(a.minimum, a.exclusive_minimum, b.minimum, b.exclusive_minimum, true);
            (a.maximum, a.exclusive_maximum) =
                bound(a.maximum, a.exclusive_maximum, b.maximum, b.exclusive_maximum, false);
            check_range(path, a.minimum, a.maximum)?;
        }
        (a, b) if a == b => {}
        _ => return Err(unsupported(path, "property types or composite constraints differ")),
    }
    return Ok(ReferenceOr::Item(Box::new(left)));
}

fn combine_keyword<T: Clone + Default + PartialEq>(path: &str, keyword: &str, left: &mut T, right: &T) -> Result<()> {
    if *left == T::default() {
        *left = right.clone();
        return Ok(());
    }
    if *right != T::default() && left != right {
        return Err(unsupported(path, &format!("specified {keyword} constraints differ")));
    }
    return Ok(());
}

fn tighter<T: Copy + PartialOrd>(a: Option<T>, b: Option<T>, minimum: bool) -> Option<T> {
    return bound(a, false, b, false, minimum).0;
}

fn bound<T: Copy + PartialOrd>(
    a: Option<T>,
    a_exclusive: bool,
    b: Option<T>,
    b_exclusive: bool,
    minimum: bool,
) -> (Option<T>, bool) {
    return match (a, b) {
        (None, _) => (b, b_exclusive),
        (_, None) => (a, a_exclusive),
        (Some(a), Some(b)) if a == b => (Some(a), a_exclusive || b_exclusive),
        (Some(a), Some(b)) if (minimum && b > a) || (!minimum && b < a) => (Some(b), b_exclusive),
        _ => (a, a_exclusive),
    };
}

fn check_range<T: PartialOrd>(path: &str, minimum: Option<T>, maximum: Option<T>) -> Result<()> {
    if let (Some(minimum), Some(maximum)) = (minimum, maximum)
        && minimum > maximum
    {
        return Err(unsupported(path, "the bounds accept no value"));
    }
    return Ok(());
}

fn narrow_enum<T: Clone + PartialEq>(path: &str, left: &mut Vec<T>, right: &[T]) -> Result<()> {
    if left.is_empty() {
        left.extend_from_slice(right);
        return Ok(());
    }
    if !right.is_empty() {
        left.retain(|value| return right.contains(value));
        if left.is_empty() {
            return Err(unsupported(path, "the enum intersection accepts no value"));
        }
    }
    return Ok(());
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn spec(schemas: serde_json::Value) -> Spec {
        let document = serde_json::from_value(json!({
            "openapi": "3.0.3", "info": {"title": "test", "version": "1"},
            "paths": {}, "components": {"schemas": schemas},
        }))
        .expect("parse document");
        return Spec::from_parts(document, "allof.yaml".into());
    }

    fn lower(schema: serde_json::Value) -> Result<crate::ir::Module> {
        let spec = spec(json!({"Test": schema}));
        let names = crate::lower::rename::type_renames(&spec, None)?;
        return crate::lower::schema::generate_models(&spec, &names);
    }

    fn object(property: serde_json::Value) -> serde_json::Value {
        return json!({"type": "object", "properties": {"value": property}});
    }

    #[test]
    fn incompatible_properties_report_context_in_both_orders() {
        for (left, right, reason) in [
            (json!({"type":"string"}), json!({"type":"integer"}), "property types"),
            (
                json!({"type":"string","minLength":5_i64}),
                json!({"type":"string","maxLength":2_i64}),
                "bounds",
            ),
            (
                json!({"type":"integer","minimum":5_i64}),
                json!({"type":"integer","maximum":2_i64}),
                "bounds",
            ),
            (
                json!({"type":"string","pattern":"a"}),
                json!({"type":"string","pattern":"b"}),
                "patterns",
            ),
            (
                json!({"type":"string","format":"uuid"}),
                json!({"type":"string","format":"date"}),
                "formats",
            ),
            (
                json!({"type":"string","enum":["a"]}),
                json!({"type":"string","enum":["b"]}),
                "enum intersection",
            ),
            (
                json!({"type":"string","readOnly":true}),
                json!({"type":"string"}),
                "metadata",
            ),
            (
                json!({"type":"string","writeOnly":true}),
                json!({"type":"string"}),
                "metadata",
            ),
            (
                json!({"type":"string","default":"a"}),
                json!({"type":"string","default":"b"}),
                "metadata",
            ),
            (
                json!({"type":"string","x-rust-name":"first"}),
                json!({"type":"string","x-rust-name":"second"}),
                "metadata",
            ),
            (
                json!({"type":"integer","multipleOf":2_i64}),
                json!({"type":"integer","multipleOf":3_i64}),
                "multipleOf",
            ),
        ] {
            for (left, right) in [(&left, &right), (&right, &left)] {
                let composition = json!({"allOf":[object(left.clone()), object(right.clone())]});
                for schema in [composition.clone(), object(composition)] {
                    let error = lower(schema).expect_err("unsupported overlap").to_string();
                    assert!(
                        error.contains("Test") && error.contains("value") && error.contains(reason),
                        "{error}"
                    );
                }
            }
        }
    }

    #[test]
    fn unsupported_member_restrictions_are_not_discarded() {
        for (member, reason) in [
            (
                json!({"type":"object","additionalProperties":{"type":"string"}}),
                "schema-valued additionalProperties",
            ),
            (json!({"type":"object","minProperties":1_i64}), "property-count"),
            (json!({"type":"object","nullable":true}), "nullability"),
            (json!({"type":"object","default":{}}), "defaults"),
            (json!({"type":"object","x-rust-type":"serde_json::Value"}), "extensions"),
        ] {
            let error = lower(json!({"allOf":[member]}))
                .expect_err("unsupported member")
                .to_string();
            assert!(error.contains("Test") && error.contains(reason), "{error}");
        }
        for required in [json!([]), json!(["value"])] {
            let other = json!({"type":"object","required":required,"properties":{"value":{"type":"string"}}});
            let closed = json!({"type":"object","additionalProperties":false});
            for members in [json!([closed, other]), json!([other, closed])] {
                let error = lower(json!({"allOf":members}))
                    .expect_err("forbidden property")
                    .to_string();
                assert!(error.contains("Test") && error.contains("closed member"), "{error}");
            }
        }
    }

    #[test]
    fn scalar_intersection_is_commutative() {
        let spec = spec(json!({}));
        for (left, right) in [
            (
                json!({"type":"number","minimum":1.5_f64,"exclusiveMinimum":true}),
                json!({"type":"number","minimum":1.5_f64,"maximum":3.5_f64}),
            ),
            (
                json!({"type":"integer","nullable":true,"minimum":1_i64}),
                json!({"type":"integer","nullable":true,"maximum":5_i64}),
            ),
            (
                json!({"type":"string","nullable":true}),
                json!({"type":"string","minLength":2_i64}),
            ),
            (
                json!({"type":"integer","enum":[1_i64,2_i64,3_i64]}),
                json!({"type":"integer","enum":[2_i64,3_i64]}),
            ),
        ] {
            let left = serde_json::from_value(left).expect("left schema");
            let right = serde_json::from_value(right).expect("right schema");
            assert_eq!(
                intersect(&spec, "Test.value", &left, &right).expect("forward"),
                intersect(&spec, "Test.value", &right, &left).expect("reverse"),
            );
        }
    }

    #[test]
    fn unspecified_keywords_preserve_the_other_members_constraints() {
        let spec = spec(json!({}));
        for (left, right, expected) in [
            (
                json!({"type":"string","pattern":"^[a-z]+$"}),
                json!({"type":"string","minLength":3_i64}),
                json!({"type":"string","pattern":"^[a-z]+$","minLength":3_i64}),
            ),
            (
                json!({"type":"string","format":"uuid"}),
                json!({"type":"string"}),
                json!({"type":"string","format":"uuid"}),
            ),
            (
                json!({"type":"integer","format":"int32"}),
                json!({"type":"integer","minimum":2_i64}),
                json!({"type":"integer","format":"int32","minimum":2_i64}),
            ),
            (
                json!({"type":"integer","multipleOf":2_i64}),
                json!({"type":"integer","minimum":0_i64}),
                json!({"type":"integer","multipleOf":2_i64,"minimum":0_i64}),
            ),
            (
                json!({"type":"number","format":"float"}),
                json!({"type":"number","maximum":10.0_f64}),
                json!({"type":"number","format":"float","maximum":10.0_f64}),
            ),
            (
                json!({"type":"number","multipleOf":0.5_f64}),
                json!({"type":"number","maximum":10.0_f64}),
                json!({"type":"number","multipleOf":0.5_f64,"maximum":10.0_f64}),
            ),
        ] {
            let left = serde_json::from_value(left).expect("left schema");
            let right = serde_json::from_value(right).expect("right schema");
            let expected = serde_json::from_value(expected).expect("intersection schema");
            for (left, right) in [(&left, &right), (&right, &left)] {
                assert_eq!(
                    intersect(&spec, "Test.value", left, right).expect("compatible intersection"),
                    expected,
                );
            }
        }
    }

    #[test]
    fn nullable_formatted_string_constraints_are_not_discarded() {
        for format in ["uuid", "date", "date-time", "byte", "binary"] {
            for constraint in [
                json!({"pattern":"^a"}),
                json!({"minLength":3_i64}),
                json!({"maxLength":10_i64}),
            ] {
                let formatted = json!({"type":"string","nullable":true,"format":format});
                let mut constrained = constraint;
                constrained["type"] = json!("string");
                constrained["nullable"] = json!(true);
                for (left, right) in [(&formatted, &constrained), (&constrained, &formatted)] {
                    let composition = json!({"allOf":[object(left.clone()),object(right.clone())]});
                    for schema in [composition.clone(), object(composition)] {
                        let error = lower(schema).expect_err("unsupported nullable constraints").to_string();
                        assert!(
                            error.contains("Test")
                                && error.contains("value")
                                && error.contains("nullable formatted-string constraints"),
                            "{error}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn recursive_members_stop_at_the_depth_guard() {
        let spec = spec(json!({"Cycle":{"allOf":[{"$ref":"#/components/schemas/Cycle"},{"type":"object"}]}}));
        let members = [ReferenceOr::Reference {
            reference: "#/components/schemas/Cycle".to_owned(),
        }];
        assert!(matches!(
            merge(&spec, "Cycle", &members),
            Err(Error::SchemaDepthExceeded { .. })
        ));
    }

    #[test]
    fn open_members_preserve_additional_properties_and_required_names() {
        let spec = spec(json!({}));
        let members = serde_json::from_value::<Vec<ReferenceOr<Schema>>>(json!([
            {"type":"object","required":["value"]},
            {"type":"object","additionalProperties":true,"properties":{"value":{"type":"string"}}}
        ]))
        .expect("members");
        let merged = merge(&spec, "Test", &members).expect("open intersection");
        assert_eq!(merged.required, ["value"]);
        assert_eq!(merged.additional_properties, Some(AdditionalProperties::Any(true)));
    }

    #[test]
    fn enum_metadata_and_nullable_enum_overlaps_are_explicit_errors() {
        for members in [
            json!([
                object(json!({"type":"string","nullable":true,"enum":["a","b"]})),
                object(json!({"type":"string","nullable":true,"enum":["b"]}))
            ]),
            json!([
                object(json!({"type":"string","enum":["a","b"],"x-enum-varnames":["First","Second"]})),
                object(json!({"type":"string","enum":["b","c"],"x-enum-varnames":["First","Second"]}))
            ]),
        ] {
            let error = lower(json!({"allOf":members}))
                .expect_err("unsupported enum intersection")
                .to_string();
            assert!(
                error.contains("Test.value") && error.contains("enum intersections"),
                "{error}"
            );
        }
    }
}
