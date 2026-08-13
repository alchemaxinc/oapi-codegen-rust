//! Read the validation keywords off a schema.
//!
//! OpenAPI gives a schema keywords that narrow the values it accepts, such as
//! `pattern` and `minimum`. The generator checks them where it deserializes, so
//! a bad value fails to parse and never reaches the handler.

use std::cmp::Ordering;

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

/// Fold an `exclusive` flag into the bound it sits beside.
///
/// A whole number above `n` is `n + 1` or more, so one inclusive bound says the
/// same thing. Holding one form keeps the type choice, the range check, and the
/// generated test in step, because each of them reads the same number.
///
/// `step` moves the bound inward: `1` for a minimum, `-1` for a maximum. A bound
/// at the end of the range cannot move, so it keeps its flag.
fn fold_bound(bound: Option<i64>, exclusive: bool, step: i64) -> (Option<i64>, bool) {
    let Some(value) = bound else {
        return (None, false);
    };
    if !exclusive {
        return (Some(value), false);
    }
    return match value.checked_add(step) {
        Some(moved) => (Some(moved), false),
        None => (Some(value), true),
    };
}

/// The lowest value an integer schema accepts, with `exclusiveMinimum` folded in.
pub(crate) fn inclusive_minimum(it: &openapiv3::IntegerType) -> Option<i64> {
    return fold_bound(it.minimum, it.exclusive_minimum, 1).0;
}

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
            let (minimum, exclusive_minimum) = fold_bound(it.minimum, it.exclusive_minimum, 1);
            let (maximum, exclusive_maximum) = fold_bound(it.maximum, it.exclusive_maximum, -1);
            found.minimum = minimum.map(Bound::Int);
            found.maximum = maximum.map(Bound::Int);
            found.exclusive_minimum = exclusive_minimum;
            found.exclusive_maximum = exclusive_maximum;
            found.folded_minimum = it.exclusive_minimum && !exclusive_minimum;
            found.folded_maximum = it.exclusive_maximum && !exclusive_maximum;
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
        found.folded_minimum = false;
    }
    if found.maximum.is_none() {
        found.exclusive_maximum = false;
        found.folded_maximum = false;
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
        SchemaKind::Type(Type::Integer(it)) if it.enumeration.is_empty() => crate::lower::schema::integer_type(it),
        SchemaKind::Type(Type::Number(nt)) if nt.enumeration.is_empty() => RustType::F64,
        _ => return None,
    };
    found.checked_as = Some(checked_as);
    return Some(found);
}

/// The lowest value a Rust integer type holds, and the highest.
///
/// `u64` reaches above where `i64` stops, and a document writes a bound as an
/// `i64`, so no bound can name the top of `u64`. That gives `None`, which keeps
/// a reachable bound out of the report below.
fn integer_limits(ty: &RustType) -> Option<(i64, Option<i64>)> {
    return match *ty {
        RustType::I32 => Some((i64::from(i32::MIN), Some(i64::from(i32::MAX)))),
        RustType::U32 => Some((0, Some(i64::from(u32::MAX)))),
        RustType::I64 => Some((i64::MIN, Some(i64::MAX))),
        RustType::U64 => Some((0, None)),
        _ => None,
    };
}

/// The value of an integer bound, or `None` for a float or an absent bound.
fn int_bound(bound: Option<Bound>) -> Option<i64> {
    return match bound {
        Some(Bound::Int(value)) => Some(value),
        Some(Bound::Float(_)) | None => None,
    };
}

/// Report a `minimum` and a `maximum` that leave nothing between them.
///
/// The two bounds cross, or they meet on one value that an `exclusive` flag then
/// takes away. Neither reading depends on the type, so a float reaches this as an
/// integer does. An integer folds its flags away first, so a folded pair that
/// crosses arrives here already crossed.
fn crossed_bounds_reason(constraints: &Constraints) -> Option<String> {
    let low = constraints.minimum?;
    let high = constraints.maximum?;
    // A `NaN` bound compares with nothing, so it gives no order and no report.
    let order = match (low, high) {
        (Bound::Int(low), Bound::Int(high)) => low.cmp(&high),
        (Bound::Float(low), Bound::Float(high)) => low.partial_cmp(&high)?,
        _ => return None,
    };
    let low_text = bound_text(low);
    let high_text = bound_text(high);
    if order == Ordering::Greater {
        return Some(format!(
            "the bounds accept no value: they allow `{low_text}` to `{high_text}`"
        ));
    }
    // Bounds that meet leave one value, and either flag takes it away.
    if order == Ordering::Equal && (constraints.exclusive_minimum || constraints.exclusive_maximum) {
        return Some(format!(
            "the bounds accept no value: they meet at `{low_text}`, which an `exclusive` flag then leaves out"
        ));
    }
    return None;
}

/// Report bounds that accept no value the field can hold.
///
/// A `minimum` above a `maximum` accepts nothing. So does an exclusive bound on
/// the first or the last value of the type, because the fold then carries it
/// past the end. Each one is a fault in the document, and the generated code
/// would refuse every value.
///
/// A bound the author writes out of range is a different fault, and it keeps the
/// message about width. Only a bound that moved is read here.
fn empty_range_reason(constraints: &Constraints, checked: &RustType) -> Option<String> {
    if let Some(reason) = crossed_bounds_reason(constraints) {
        return Some(reason);
    }
    let (type_low, type_high) = integer_limits(checked)?;
    let minimum = int_bound(constraints.minimum);
    let maximum = int_bound(constraints.maximum);
    let label = checked.label();
    // A folded bound past the end of the type accepts nothing, and the number
    // the author wrote is one step back. A flag still set here had no room to
    // fold, so it sits at the end of `i64` and the author wrote that number.
    let stops_above = |written: i64| {
        return Some(format!(
            "the bounds accept no value: nothing lies above `{written}`, where `{label}` stops"
        ));
    };
    let starts_below = |written: i64| {
        return Some(format!(
            "the bounds accept no value: nothing lies below `{written}`, where `{label}` starts"
        ));
    };
    if let Some(low) = minimum
        && let Some(top) = type_high
    {
        if constraints.folded_minimum && low > top {
            return stops_above(low - 1);
        }
        if constraints.exclusive_minimum && low >= top {
            return stops_above(low);
        }
    }
    if let Some(high) = maximum {
        if constraints.folded_maximum && high < type_low {
            return starts_below(high + 1);
        }
        if constraints.exclusive_maximum && high <= type_low {
            return starts_below(high);
        }
    }
    return None;
}

/// Reject a keyword value the generated code cannot hold.
///
/// A `multipleOf` of zero divides by zero. For an integer that is a panic the
/// compiler already refuses; for a float it makes the check read `inf`, and the
/// comparison then reads `NaN` and never fires. Both are worse than an error
/// here. A bound outside the range of a narrow `repr`, and a pair of bounds that
/// meet nowhere, each make code that refuses every value.
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
    if let Some(step) = constraints.multiple_of {
        let positive = match step {
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
    // An empty range names the fault on its own. A bound the type cannot hold is
    // the same fault seen from further away, so only one of the two is reported.
    match empty_range_reason(constraints, checked) {
        Some(reason) => diagnostics.push(Error::UnsupportedSchema {
            path: name.to_owned(),
            reason,
        }),
        None => check_width(constraints, checked, name, &mut diagnostics),
    }
    check_reach(constraints, checked, name, &mut diagnostics);
    return diagnostics.into_result();
}

/// Reject a bound too wide for the type the field holds.
///
/// A bound outside the range of a narrow type writes a literal that does not
/// fit, the same fault an out-of-range `enum` value makes.
fn check_width(
    constraints: &Constraints,
    checked: &RustType,
    name: &str,
    diagnostics: &mut crate::lower::validate::Diagnostics,
) {
    if !matches!(*checked, RustType::I32 | RustType::U32 | RustType::U64) {
        return;
    }
    for (keyword, bound) in [
        ("minimum", constraints.minimum),
        ("maximum", constraints.maximum),
        ("multipleOf", constraints.multiple_of),
    ] {
        let Some(Bound::Int(value)) = bound else {
            continue;
        };
        // A `multipleOf` at zero or below is reported on its own above. An
        // unsigned type would report it a second time here, for a width the
        // author must not fix by widening the type.
        if keyword == "multipleOf" && value <= 0 {
            continue;
        }
        let fits = match *checked {
            RustType::I32 => i32::try_from(value).is_ok(),
            RustType::U32 => u32::try_from(value).is_ok(),
            RustType::U64 => u64::try_from(value).is_ok(),
            _ => true,
        };
        if !fits {
            diagnostics.push(Error::UnsupportedSchema {
                path: name.to_owned(),
                reason: format!("the `{keyword}` value `{value}` does not fit `{}`", checked.label()),
            });
        }
    }
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
fn bound_text(bound: Bound) -> String {
    return match bound {
        Bound::Int(value) => format!("{value}"),
        Bound::Float(value) => format!("{value}"),
    };
}
