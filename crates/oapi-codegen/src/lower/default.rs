//! Lowering a schema's `default` into a value the emitter can render.
//!
//! JSON alone does not say what Rust to write. `1` is `1` for an integer field
//! and `1.0` for a number field. `"active"` is a string for a `String` field and
//! a variant path for an enum. This pass settles that against the field type,
//! while the schema is still in hand. The emitter then prints the result.
//!
//! A value with no literal form is an error, not a silent drop. A dropped
//! default leaves the document and the code in disagreement.
//!
//! `default: null` never arrives. The parser reads it as no default at all, and
//! serde already leaves a missing `Option` as `None`.

use serde_json::Value;

use crate::error::Error;
use crate::error::Result;
use crate::ir::DefaultValue;
use crate::ir::RustType;
use crate::ir::StringVariant;

/// Lower a `default` against the type of the property.
///
/// `variants_of` finds a generated string enum by the name in
/// [`RustType::Named`]. An enum default needs this to find its variant.
///
/// Only an inline enum arrives here. In OpenAPI 3.0 a bare `$ref` drops the keys
/// beside it, so such a property has no `default`.
pub fn lower_default(
    json: &Value,
    ty: &RustType,
    variants_of: &dyn Fn(&str) -> Option<Vec<StringVariant>>,
    owner: &str,
    property: &str,
) -> Result<DefaultValue> {
    return match value_for(json, ty, variants_of) {
        Some(value) => Ok(value),
        None => Err(Error::UnsupportedDefault {
            owner: owner.to_owned(),
            property: property.to_owned(),
            declared: json.to_string(),
            hint: format!(
                "`{owner}.{property}` lowers to {}. Give it a `default` of that type, or remove the `default`. \
                 This generator renders a scalar, an enum value, an empty array, and an empty object. \
                 It cannot render a non-empty array or object.",
                describe(ty)
            ),
        }),
    };
}

/// The lowered value, or `None` when the type and the JSON disagree.
fn value_for(
    json: &Value,
    ty: &RustType,
    variants_of: &dyn Fn(&str) -> Option<Vec<StringVariant>>,
) -> Option<DefaultValue> {
    return match ty {
        // A `nullable` property keeps its `Option`. The default fills the
        // `Some` side of it.
        RustType::Option(inner) => value_for(json, inner, variants_of),
        RustType::Boxed(inner) => value_for(json, inner, variants_of),
        RustType::Bool => json.as_bool().map(DefaultValue::Bool),
        RustType::I64 => json.as_i64().map(DefaultValue::Int),
        // The emitted literal has no suffix, so `i32` takes its range from the
        // return type of the function. A value outside that range gives code
        // that does not compile.
        RustType::I32 => json
            .as_i64()
            .filter(|number| return i32::try_from(*number).is_ok())
            .map(DefaultValue::Int),
        // An integer is a valid floating-point default, and JSON writes `1`
        // rather than `1.0` for a whole number.
        RustType::F64 => json.as_f64().map(DefaultValue::Float),
        RustType::String => json.as_str().map(|text| return DefaultValue::Str(text.to_owned())),
        RustType::Vec(_) => empty_if(json.as_array().is_some_and(|items| return items.is_empty())),
        RustType::Map(_) => empty_if(json.as_object().is_some_and(|entries| return entries.is_empty())),
        RustType::Named(name) => variant_for(json, name, variants_of),
        // A date, a UUID, and the rest parse from a string at run time. There
        // is no literal to write for them.
        _ => None,
    };
}

/// [`DefaultValue::Empty`] when the collection is empty, nothing otherwise.
fn empty_if(is_empty: bool) -> Option<DefaultValue> {
    return if is_empty { Some(DefaultValue::Empty) } else { None };
}

/// The string enum variant whose wire value the default names.
///
/// The match is on the wire value, not on the identifier. This is what lets
/// `default: "in-progress"` find `InProgress`.
fn variant_for(
    json: &Value,
    name: &str,
    variants_of: &dyn Fn(&str) -> Option<Vec<StringVariant>>,
) -> Option<DefaultValue> {
    let wanted = json.as_str()?;
    let variants = variants_of(name)?;
    let found = variants.iter().find(|variant| {
        let wire = variant
            .rename
            .as_deref()
            .unwrap_or_else(|| return variant.name.logical());
        return wire == wanted;
    })?;
    return Some(DefaultValue::Variant(found.name.clone()));
}

/// The type, in the words a specification author uses, for the error message.
fn describe(ty: &RustType) -> String {
    return match ty {
        RustType::Option(inner) | RustType::Boxed(inner) => describe(inner),
        RustType::Bool => "a boolean".to_owned(),
        RustType::I32 => "a 32-bit integer".to_owned(),
        RustType::I64 => "an integer".to_owned(),
        RustType::F64 => "a number".to_owned(),
        RustType::String => "a string".to_owned(),
        RustType::Vec(_) => "an array".to_owned(),
        RustType::Map(_) => "an object".to_owned(),
        RustType::Named(name) => format!("`{name}`"),
        _ => "a type with no literal form".to_owned(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naming::Case;
    use crate::naming::to_ident;

    /// One enum, `Status`, whose only variant renames to a wire value that is
    /// not a valid identifier.
    fn status_variants(name: &str) -> Option<Vec<StringVariant>> {
        if name != "Status" {
            return None;
        }
        return Some(vec![StringVariant {
            name: to_ident("InProgress", Case::Pascal),
            rename: Some("in-progress".to_owned()),
            doc: None,
        }]);
    }

    fn lower(json: Value, ty: &RustType) -> Result<DefaultValue> {
        return lower_default(&json, ty, &status_variants, "Widget", "field");
    }

    fn lowered(json: Value, ty: &RustType) -> DefaultValue {
        return lower(json, ty).expect("this default matches the field type");
    }

    #[test]
    fn a_scalar_lowers_to_a_literal_of_the_fields_type() {
        let cases = [
            (Value::from(true), RustType::Bool, DefaultValue::Bool(true)),
            (Value::from(7_i64), RustType::I64, DefaultValue::Int(7)),
            (Value::from(7_i64), RustType::F64, DefaultValue::Float(7.0_f64)),
            (Value::from(1.5_f64), RustType::F64, DefaultValue::Float(1.5_f64)),
            (Value::from("hi"), RustType::String, DefaultValue::Str("hi".to_owned())),
        ];
        for (json, ty, expected) in cases {
            assert_eq!(lowered(json, &ty), expected);
        }
    }

    #[test]
    fn an_enum_default_matches_on_the_wire_value_not_the_identifier() {
        let ty = RustType::Named("Status".to_owned());
        assert_eq!(
            lowered(Value::from("in-progress"), &ty),
            DefaultValue::Variant(to_ident("InProgress", Case::Pascal))
        );
        assert!(lower(Value::from("InProgress"), &ty).is_err());
    }

    #[test]
    fn only_an_empty_collection_lowers() {
        let vec_ty = RustType::Vec(Box::new(RustType::String));
        assert_eq!(lowered(Value::from(Vec::<String>::new()), &vec_ty), DefaultValue::Empty);
        assert!(lower(Value::from(vec!["a"]), &vec_ty).is_err());
    }

    #[test]
    fn a_nullable_field_defaults_to_the_some_side() {
        let optional = RustType::Option(Box::new(RustType::String));
        assert_eq!(
            lowered(Value::from("hi"), &optional),
            DefaultValue::Str("hi".to_owned())
        );
    }

    #[test]
    fn a_value_of_the_wrong_type_is_rejected() {
        assert!(lower(Value::from("7"), &RustType::I64).is_err());
        assert!(lower(Value::from(7_i64), &RustType::String).is_err());
        assert!(lower(Value::from(1.5_f64), &RustType::I64).is_err());
    }

    #[test]
    fn an_i32_default_outside_the_range_is_rejected() {
        let cases = [
            (Value::from(i64::from(i32::MAX)), true),
            (Value::from(i64::from(i32::MIN)), true),
            (Value::from(i64::from(i32::MAX) + 1_i64), false),
            (Value::from(i64::from(i32::MIN) - 1_i64), false),
        ];
        for (json, is_ok) in cases {
            assert_eq!(lower(json, &RustType::I32).is_ok(), is_ok);
        }
    }

    #[test]
    fn a_type_with_no_literal_form_is_rejected() {
        assert!(lower(Value::from("2020-01-01"), &RustType::Date).is_err());
        // An empty map has a literal, `Default::default()`. A struct has none,
        // so `{}` works for the first and not for the second.
        let map = RustType::Map(Box::new(RustType::String));
        assert_eq!(
            lowered(Value::Object(serde_json::Map::new()), &map),
            DefaultValue::Empty
        );
        let named = RustType::Named("Status".to_owned());
        assert!(lower(Value::Object(serde_json::Map::new()), &named).is_err());
    }
}
