//! Read the validation keywords off a schema.
//!
//! OpenAPI gives a schema keywords that narrow the values it accepts, such as
//! `pattern` and `minimum`. The generator checks them where it deserializes, so
//! a bad value fails to parse and never reaches the handler.

use openapiv3::Schema;
use openapiv3::SchemaKind;
use openapiv3::Type;

use crate::error::Error;
use crate::error::Result;
use crate::ir::Bound;
use crate::ir::Constraints;
use crate::ir::Field;
use crate::ir::RustType;
use crate::lower::schema::X_RUST_TYPE;

/// The validation keywords a schema declares, or `None` when it declares none.
///
/// A keyword the generator cannot check on its own type is left alone. The
/// parser accepts `minLength` on an integer, for example, and nothing reads it.
pub(crate) fn constraints_of(schema: &Schema) -> Option<Constraints> {
    let mut found = Constraints::default();
    match &schema.schema_kind {
        SchemaKind::Type(Type::String(st)) => {
            found.pattern = st.pattern.clone();
            found.min_length = st.min_length;
            found.max_length = st.max_length;
        }
        SchemaKind::Type(Type::Integer(it)) => {
            found.minimum = it.minimum.map(Bound::Int);
            found.maximum = it.maximum.map(Bound::Int);
            found.exclusive_minimum = it.exclusive_minimum;
            found.exclusive_maximum = it.exclusive_maximum;
            found.multiple_of = it.multiple_of.map(Bound::Int);
        }
        SchemaKind::Type(Type::Number(nt)) => {
            found.minimum = nt.minimum.map(Bound::Float);
            found.maximum = nt.maximum.map(Bound::Float);
            found.exclusive_minimum = nt.exclusive_minimum;
            found.exclusive_maximum = nt.exclusive_maximum;
            found.multiple_of = nt.multiple_of.map(Bound::Float);
        }
        SchemaKind::Type(Type::Array(at)) => {
            found.min_items = at.min_items;
            found.max_items = at.max_items;
            found.unique_items = at.unique_items;
        }
        SchemaKind::Type(Type::Object(ot)) => {
            found.min_properties = ot.min_properties;
            found.max_properties = ot.max_properties;
        }
        SchemaKind::Type(Type::Boolean(_))
        | SchemaKind::OneOf { .. }
        | SchemaKind::AnyOf { .. }
        | SchemaKind::AllOf { .. }
        | SchemaKind::Not { .. }
        | SchemaKind::Any(_) => {}
    }
    // `exclusiveMinimum` without `minimum` bounds nothing. OpenAPI 3.0 writes it
    // as a flag beside the bound, so a lone flag is a no-op and not an error.
    if found.minimum.is_none() {
        found.exclusive_minimum = false;
    }
    if found.maximum.is_none() {
        found.exclusive_maximum = false;
    }
    if found.is_empty() {
        return None;
    }
    return Some(found);
}

/// The keywords a `$ref` target gives a field, and the type they check.
///
/// A `$ref` to a constrained scalar makes a type alias, and an alias carries no
/// serde attribute. So the field that names it takes the checks instead. Only a
/// plain scalar qualifies: an `enum` or an `x-rust-type` makes the alias name
/// something other than the type below it, and nothing here would fit.
pub(crate) fn constraints_through_ref(target: &Schema) -> Option<Constraints> {
    let mut found = constraints_of(target)?;
    if target.schema_data.extensions.contains_key(X_RUST_TYPE) {
        return None;
    }
    let checked_as = match &target.schema_kind {
        SchemaKind::Type(Type::String(st)) if st.enumeration.is_empty() => {
            crate::lower::schema::string_format_type(&st.format)
        }
        SchemaKind::Type(Type::Integer(it)) if it.enumeration.is_empty() => {
            crate::lower::schema::integer_format_type(&it.format)
        }
        SchemaKind::Type(Type::Number(nt)) if nt.enumeration.is_empty() => RustType::F64,
        _ => return None,
    };
    found.checked_as = Some(checked_as);
    return Some(found);
}

/// Reject a keyword value the generated code cannot hold.
///
/// A `multipleOf` of zero divides by zero. For an integer that is a panic the
/// compiler already refuses; for a float it makes the check read `inf`, and the
/// comparison then reads `NaN` and never fires. Both are worse than an error
/// here. A bound outside the range of a narrow `repr` writes a literal that does
/// not fit, the same fault an out-of-range `enum` value makes.
///
/// # Errors
///
/// Returns [`Error::UnsupportedSchema`] for each keyword the field cannot hold.
pub(crate) fn check_constraints(field: &Field) -> Result<()> {
    let Some(constraints) = &field.constraints else {
        return Ok(());
    };
    let checked = match &constraints.checked_as {
        Some(ty) => ty,
        None => field.ty.innermost(),
    };
    let name = field.name.logical();
    let mut diagnostics = crate::lower::validate::Diagnostics::new();
    if let Some(step) = &constraints.multiple_of {
        let positive = match *step {
            Bound::Int(value) => value > 0,
            Bound::Float(value) => value > 0.0_f64,
        };
        if !positive {
            diagnostics.push(Error::UnsupportedSchema {
                path: name.to_owned(),
                reason: format!("the `multipleOf` value `{}` is not above zero", bound_text(step)),
            });
        }
    }
    if matches!(*checked, RustType::I32) {
        for (keyword, bound) in [
            ("minimum", &constraints.minimum),
            ("maximum", &constraints.maximum),
            ("multipleOf", &constraints.multiple_of),
        ] {
            let Some(Bound::Int(value)) = *bound else {
                continue;
            };
            if i32::try_from(value).is_err() {
                diagnostics.push(Error::UnsupportedSchema {
                    path: name.to_owned(),
                    reason: format!("the `{keyword}` value `{value}` does not fit `i32`"),
                });
            }
        }
    }
    check_reach(constraints, checked, name, &mut diagnostics);
    return diagnostics.into_result();
}

/// Reject a keyword that cannot reach the type the field holds.
///
/// A `format` can name a type that is no longer a string, such as a date or a
/// UUID, and a `pattern` has nothing to read once the value is parsed. Leaving
/// the keyword alone would tell the reader of the document that a rule runs when
/// none does, which is the fault this whole section removes. So say so, and name
/// the way out.
fn check_reach(
    constraints: &Constraints,
    checked: &RustType,
    name: &str,
    diagnostics: &mut crate::lower::validate::Diagnostics,
) {
    let mut unreachable = |keyword: &str| {
        diagnostics.push(Error::UnsupportedSchema {
            path: name.to_owned(),
            reason: format!("the `{keyword}` rule does not reach the type this field holds"),
        });
    };
    if !matches!(*checked, RustType::String) {
        for (keyword, present) in [
            ("pattern", constraints.pattern.is_some()),
            ("minLength", constraints.min_length.is_some_and(|min| return min > 0)),
            ("maxLength", constraints.max_length.is_some()),
        ] {
            if present {
                unreachable(keyword);
            }
        }
    }
    if !matches!(*checked, RustType::Map(_)) {
        for (keyword, present) in [
            (
                "minProperties",
                constraints.min_properties.is_some_and(|min| return min > 0),
            ),
            ("maxProperties", constraints.max_properties.is_some()),
        ] {
            if present {
                unreachable(keyword);
            }
        }
    }
    // A list of models can lose `PartialEq` to F.1, and the comparison needs it.
    let comparable = matches!(*checked, RustType::Vec(ref element) if element.is_scalar());
    if constraints.unique_items && !comparable {
        unreachable("uniqueItems");
    }
}

/// A bound as a message writes it.
fn bound_text(bound: &Bound) -> String {
    return match *bound {
        Bound::Int(value) => format!("{value}"),
        Bound::Float(value) => format!("{value}"),
    };
}
