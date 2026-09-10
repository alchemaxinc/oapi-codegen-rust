//! Render the checks a schema's validation keywords ask for.
//!
//! A check runs during deserialization, so a value that breaks a rule fails to
//! parse. On a server that gives a 400, and the handler never sees the value.
//! Nothing checks a value the generated code writes, because the code that
//! builds it owns it.

use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;

use super::emit_type;
use crate::error::Result;
use crate::ir::Bound;
use crate::ir::Constraints;
use crate::ir::Field;
use crate::ir::RustType;

/// The name of the function behind `#[serde(deserialize_with = "..")]`.
pub(crate) fn validate_fn_name(field: &Field) -> proc_macro2::Ident {
    return format_ident!("validate_{}", field.name.logical());
}

/// Whether a field carries a keyword the generator can check on its type.
///
/// A keyword the type cannot use is dropped here. `minLength` on an integer
/// parses, and means nothing.
pub(crate) fn is_checked(field: &Field) -> bool {
    let Some(constraints) = &field.constraints else {
        return false;
    };
    return !checks(constraints, checked_type(field, constraints)).is_empty();
}

/// Render the function behind `#[serde(deserialize_with = "..")]`.
pub(crate) fn emit_validate_fn(field: &Field) -> Result<TokenStream> {
    let name = validate_fn_name(field);
    let ty = emit_type(&field.ty)?;
    let empty = Constraints::default();
    let constraints = field.constraints.as_ref().unwrap_or(&empty);
    let tests = if crate::lower::constraints::accepts_only_null(field) {
        vec![Check {
            test: quote! {{ let _ = item; true }},
            message: "has no non-null value within the declared bounds".to_owned(),
        }]
    } else {
        checks(constraints, checked_type(field, constraints))
    };
    let body = wrap(&field.ty, &tests, field.name.logical(), constraints.checked_as.as_ref());
    let read = match &field.ty {
        RustType::Option(inner) => {
            let inner = emit_type(inner)?;
            quote! { Some(<#inner as serde::Deserialize>::deserialize(deserializer)?) }
        }
        _ => quote! { <#ty as serde::Deserialize>::deserialize(deserializer)? },
    };
    let pattern = emit_pattern(field, constraints)?;
    let doc = format!(
        " The rules the document gives `{}`, checked on the way in.",
        field.name.logical()
    );
    return Ok(quote! {
        #[doc = #doc]
        fn #name<'de, D>(deserializer: D) -> ::core::result::Result<#ty, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            #pattern
            let value = #read;
            #body
            return Ok(value);
        }
    });
}

/// One rule, as the test that rejects a value and the message it gives.
struct Check {
    /// The test, over a binding named `item`. True means the value is bad.
    test: TokenStream,
    /// The message the error carries.
    message: String,
}

/// Walk the wrappers around the value, so a rule reads the value it names.
///
/// An `Option` runs the rules only when it holds a value. A `Vec` carries its
/// own rules, and is not walked into, because `items` is a schema of its own.
fn wrap(ty: &RustType, tests: &[Check], label: &str, checked_as: Option<&RustType>) -> TokenStream {
    if tests.is_empty() {
        return quote! {};
    }
    let rules = tests.iter().map(|check| {
        let test = &check.test;
        let message = format!("`{label}` {}", check.message);
        return quote! {
            if #test {
                return Err(serde::de::Error::custom(#message));
            }
        };
    });
    let rules: Vec<TokenStream> = rules.collect();
    let body = unwrap_checks(ty, quote! { #(#rules)* }, checked_as);
    return quote! {{ let item = &value; #body }};
}

fn unwrap_checks(ty: &RustType, body: TokenStream, checked_as: Option<&RustType>) -> TokenStream {
    return match ty {
        RustType::Option(inner) | RustType::Nullable(inner) => {
            let body = unwrap_checks(inner, body, checked_as);
            quote! { if let Some(item) = item.as_ref() { #body } }
        }
        RustType::Boxed(inner) => {
            let body = unwrap_checks(inner, body, checked_as);
            quote! {{ let item = item.as_ref(); #body }}
        }
        RustType::Named(_) => match checked_as {
            Some(checked) => unwrap_checks(checked, body, None),
            _ => body,
        },
        _ => body,
    };
}

/// The rules a set of keywords asks for, on the type they apply to.
/// The type the checks run against.
///
/// A `$ref` names an alias, and the name says nothing about the type below it.
/// Lowering resolves that type, so read it when it is there.
fn checked_type<'a>(field: &'a Field, constraints: &'a Constraints) -> &'a RustType {
    return match &constraints.checked_as {
        Some(ty) => ty.innermost(),
        None => field.ty.innermost(),
    };
}

fn checks(constraints: &Constraints, ty: &RustType) -> Vec<Check> {
    let mut tests = Vec::new();
    if matches!(*ty, RustType::String) {
        string_checks(constraints, &mut tests);
    }
    if matches!(
        *ty,
        RustType::I32 | RustType::I64 | RustType::U32 | RustType::U64 | RustType::F64
    ) {
        number_checks(constraints, ty, &mut tests);
    }
    if matches!(*ty, RustType::Vec(_)) {
        array_checks(constraints, ty, &mut tests);
    }
    if matches!(*ty, RustType::Map(_)) {
        map_checks(constraints, &mut tests);
    }
    return tests;
}

/// The `regex` a `pattern` needs, built one time and held for later calls.
///
/// The generator compiles the pattern too, and rejects one this crate cannot
/// read. So the `expect` here cannot fire.
fn emit_pattern(field: &Field, constraints: &Constraints) -> Result<TokenStream> {
    let Some(pattern) = &constraints.pattern else {
        return Ok(quote! {});
    };
    if !matches!(*checked_type(field, constraints), RustType::String) {
        return Ok(quote! {});
    }
    if let Err(problem) = regex::Regex::new(pattern) {
        return Err(crate::error::Error::UnsupportedSchema {
            path: field.name.logical().to_owned(),
            reason: format!("the `pattern` `{pattern}` does not read as a regular expression: {problem}"),
        });
    }
    let note = format!("the generator read `{pattern}` at generation time");
    return Ok(quote! {
        static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let pattern = PATTERN.get_or_init(|| return regex::Regex::new(#pattern).expect(#note));
    });
}

/// `pattern`, `minLength`, and `maxLength`. A length counts characters as JSON
/// Schema counts them, and not bytes. A multi-byte character counts once.
fn string_checks(constraints: &Constraints, tests: &mut Vec<Check>) {
    if let Some(pattern) = &constraints.pattern {
        tests.push(Check {
            test: quote! { !pattern.is_match(item) },
            message: format!("must match `{pattern}`"),
        });
    }
    // A length reads characters, not bytes, as JSON Schema states. Reading one
    // character past the bound answers the question, so a long string from an
    // untrusted caller costs the bound and not its own length.
    if let Some(min) = constraints.min_length.filter(|min| return *min > 0) {
        let last = min - 1;
        tests.push(Check {
            test: quote! { item.chars().nth(#last).is_none() },
            message: format!("must hold {min} or more characters"),
        });
    }
    if let Some(max) = constraints.max_length {
        tests.push(Check {
            test: quote! { item.chars().nth(#max).is_some() },
            message: format!("must hold {max} or fewer characters"),
        });
    }
}

/// The lowest and highest value a type holds, where a bound keyword can name it.
///
/// `None` says a bound cannot reach that end. `u64` holds values above
/// `i64::MAX`, and no bound keyword writes a number that high, so a `maximum`
/// never covers the whole type.
fn type_limits(ty: &RustType) -> (Option<i64>, Option<i64>) {
    return match *ty {
        RustType::I32 => (Some(i64::from(i32::MIN)), Some(i64::from(i32::MAX))),
        RustType::I64 => (Some(i64::MIN), Some(i64::MAX)),
        RustType::U32 => (Some(0), Some(i64::from(u32::MAX))),
        RustType::U64 => (Some(0), None),
        _ => (None, None),
    };
}

/// `minimum`, `maximum`, and `multipleOf`, with the two `exclusive` flags.
fn number_checks(constraints: &Constraints, ty: &RustType, tests: &mut Vec<Check>) {
    let (lowest, highest) = type_limits(ty);
    if let Some(min) = constraints.minimum {
        let literal = bound_literal(min);
        let text = bound_text(min);
        // A type that already refuses every value below the bound makes a test
        // no value fails. Rust reads `x < 0` on an unsigned type as a warning,
        // and the reader gains nothing from it.
        let no_value_fails = !constraints.exclusive_minimum
            && matches!(min, Bound::Int(value) if lowest.is_some_and(|lowest| return value <= lowest));
        let test = if constraints.exclusive_minimum {
            Some(Check {
                test: quote! { *item <= #literal },
                message: format!("must be more than {text}"),
            })
        } else if no_value_fails {
            None
        } else {
            Some(Check {
                test: quote! { *item < #literal },
                message: format!("must be {text} or more"),
            })
        };
        if let Some(check) = test {
            tests.push(check);
        }
    }
    if let Some(max) = constraints.maximum {
        let literal = bound_literal(max);
        let text = bound_text(max);
        let no_value_fails = !constraints.exclusive_maximum
            && matches!(max, Bound::Int(value) if highest.is_some_and(|highest| return value >= highest));
        let test = if constraints.exclusive_maximum {
            Some(Check {
                test: quote! { *item >= #literal },
                message: format!("must be less than {text}"),
            })
        } else if no_value_fails {
            None
        } else {
            Some(Check {
                test: quote! { *item > #literal },
                message: format!("must be {text} or less"),
            })
        };
        if let Some(check) = test {
            tests.push(check);
        }
    }
    if let Some(step) = constraints.multiple_of {
        let literal = bound_literal(step);
        let text = bound_text(step);
        if matches!(*ty, RustType::F64) {
            // A float divide does not land on a whole number. `0.3 / 0.1` gives
            // 2.999999999999999_6, and a plain test on the remainder rejects a
            // value the document accepts. So round, then measure the distance.
            // The room to move grows with the count, as the float error does.
            tests.push(Check {
                test: quote! {{
                    let steps = *item / #literal;
                    (steps - steps.round()).abs() > f64::EPSILON * steps.abs().max(1.0) * 8.0
                }},
                message: format!("must be a multiple of {text}"),
            });
        } else {
            tests.push(Check {
                test: quote! { *item % #literal != 0 },
                message: format!("must be a multiple of {text}"),
            });
        }
    }
}

/// `minItems`, `maxItems`, and `uniqueItems`.
///
/// A hashable element gets a set, which reads the list one time. A `f64` has no
/// `Eq` and no `Hash`, so it falls back to a comparison of each pair. That pair
/// test costs the square of the length, and a server reads this list from an
/// untrusted caller, so `maxItems` beside `uniqueItems` is worth writing.
fn array_checks(constraints: &Constraints, ty: &RustType, tests: &mut Vec<Check>) {
    if let Some(min) = constraints.min_items {
        tests.push(Check {
            test: quote! { item.len() < #min },
            message: format!("must hold {min} or more items"),
        });
    }
    if let Some(max) = constraints.max_items {
        tests.push(Check {
            test: quote! { item.len() > #max },
            message: format!("must hold {max} or fewer items"),
        });
    }
    if !constraints.unique_items {
        return;
    }
    let RustType::Vec(ref element) = *ty else {
        return;
    };
    // A model of our own can lose `PartialEq` to F.1, and nothing here would
    // hold. A scalar keeps every trait it needs.
    if !element.is_scalar() {
        return;
    }
    let test = if matches!(element.innermost(), RustType::F64) {
        quote! {
            item.iter().enumerate().any(|(index, left)| {
                return item.iter().skip(index + 1).any(|right| return left == right);
            })
        }
    } else {
        quote! {{
            let mut seen = std::collections::HashSet::with_capacity(item.len());
            item.iter().any(|entry| return !seen.insert(entry))
        }}
    };
    tests.push(Check {
        test,
        message: "must not repeat an item".to_owned(),
    });
}

/// `minProperties` and `maxProperties`, which reach a map and not a struct.
///
/// A struct names its properties, so the count is fixed when the code compiles
/// and a check would read the same answer every time.
fn map_checks(constraints: &Constraints, tests: &mut Vec<Check>) {
    if let Some(min) = constraints.min_properties {
        tests.push(Check {
            test: quote! { item.len() < #min },
            message: format!("must hold {min} or more properties"),
        });
    }
    if let Some(max) = constraints.max_properties {
        tests.push(Check {
            test: quote! { item.len() > #max },
            message: format!("must hold {max} or fewer properties"),
        });
    }
}

/// A bound as a literal of its own type.
///
/// The bound and the field agree by construction. An `integer` schema gives an
/// `i32` or an `i64` and an integer bound, and a `number` schema gives an `f64`
/// and a float bound. So the literal follows the bound, and nothing converts.
fn bound_literal(bound: Bound) -> TokenStream {
    return match bound {
        Bound::Float(value) => {
            let literal = proc_macro2::Literal::f64_suffixed(value);
            quote! { #literal }
        }
        Bound::Int(value) => {
            let literal = proc_macro2::Literal::i64_unsuffixed(value);
            quote! { #literal }
        }
    };
}

/// A bound as the message writes it.
fn bound_text(bound: Bound) -> String {
    return match bound {
        Bound::Int(value) => value.to_string(),
        Bound::Float(value) => value.to_string(),
    };
}
