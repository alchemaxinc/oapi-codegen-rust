use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use serde_json::Value;

use super::models::ModelDerives;
use super::models::SerdeDerives;
use super::models::deprecated_attr;
use super::models::derive_attr;
use crate::emit::doc_attr;
use crate::emit::emit_type;
use crate::error::Result;
use crate::ir::Enum;
use crate::ir::UnionValidationNode;
use crate::ir::UnionVariant;
use crate::naming::Case;
use crate::naming::to_ident;

const MAX_VALIDATION_DEPTH: usize = 128;
const MAX_VALIDATION_WORK: usize = 10_000;

pub(super) fn emit(enom: &Enum, variants: &[UnionVariant], any_of: bool, derives: ModelDerives) -> Result<TokenStream> {
    let name = enom.name.to_token();
    let doc = doc_attr(&enom.doc);
    let deprecated = deprecated_attr(&enom.deprecated);
    let max_depth = MAX_VALIDATION_DEPTH;
    let max_work = MAX_VALIDATION_WORK;
    let mut helpers = Vec::new();
    let mut checks = Vec::new();
    let mut payloads = Vec::new();
    let mut decode = Vec::new();
    let mut accessors = Vec::new();
    for (index, variant) in variants.iter().enumerate() {
        let variant_name = variant.name.to_token();
        let ty = emit_type(&variant.ty)?;
        let root = helper(index, 0);
        checks.push(quote! { Self::#root(&value, 0, &mut budget) });
        payloads.push(quote! { #variant_name(#ty), });
        decode.push(quote! {
            #index => <#ty as serde::Deserialize>::deserialize(value)
                .map(Self::#variant_name).map_err(serde::de::Error::custom),
        });
        if derives.serde.deserialize {
            let accessor = format_ident!("as_{}", to_ident(variant.name.logical(), Case::Snake).logical());
            let accessor_doc = format!(
                "Decode the `{}` alternative. Return `None` when its schema does not match.",
                variant.name.logical()
            );
            accessors.push(quote! {
                #[doc = #accessor_doc]
                pub fn #accessor(&self) -> ::std::result::Result<::std::option::Option<#ty>, serde_json::Error> {
                    let mut budget = #max_work;
                    let matched = Self::#root(&self.value, 0, &mut budget);
                    if budget == 0 {
                        return ::std::result::Result::Err(<serde_json::Error as serde::de::Error>::custom("union validation limit exceeded"));
                    }
                    if !matched {
                        return ::std::result::Result::Ok(::std::option::Option::None);
                    }
                    return <#ty as serde::Deserialize>::deserialize(&self.value).map(::std::option::Option::Some);
                }
            });
        }
        for (node_index, node) in variant.validation.nodes.iter().enumerate() {
            let ident = helper(index, node_index);
            let predicate = predicate(index, node);
            helpers.push(quote! {
                fn #ident(value: &serde_json::Value, depth: usize, budget: &mut usize) -> bool {
                    if depth > #max_depth || *budget <= 1 {
                        *budget = 0;
                        return false;
                    }
                    *budget -= 1;
                    let _ = value;
                    return #predicate;
                }
            });
        }
    }
    if variants
        .iter()
        .flat_map(|variant| return &variant.validation.nodes)
        .any(|node| {
            return node.keywords.contains_key("enum") || node.keywords.get("uniqueItems") == Some(&Value::Bool(true));
        })
    {
        helpers.push(schema_equality());
    }
    if variants
        .iter()
        .flat_map(|variant| return &variant.validation.nodes)
        .any(|node| return node.keywords.contains_key("multipleOf"))
    {
        helpers.push(decimal_multiple());
    }
    let count = quote! { [#(#checks),*].into_iter().filter(|matched| *matched).count() };
    if any_of {
        let derive = derive_attr(ModelDerives {
            serde: SerdeDerives {
                serialize: derives.serde.serialize,
                deserialize: false,
            },
            ..derives
        });
        let transparent = if derives.serde.serialize {
            quote! { #[serde(transparent)] }
        } else {
            quote! {}
        };
        let deserialize = if derives.serde.deserialize {
            quote! {
                impl<'de> serde::Deserialize<'de> for #name {
                    fn deserialize<__Deserializer: serde::Deserializer<'de>>(deserializer: __Deserializer) -> ::std::result::Result<Self, __Deserializer::Error> {
                        let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
                        return <Self as ::std::convert::TryFrom<serde_json::Value>>::try_from(value).map_err(serde::de::Error::custom);
                    }
                }
            }
        } else {
            quote! {}
        };
        return Ok(quote! {
            #doc
            #derive
            #transparent
            #deprecated
            pub struct #name {
                value: serde_json::Value,
            }

            impl #name {
                /// Borrow the complete validated JSON value.
                pub fn as_value(&self) -> &serde_json::Value {
                    return &self.value;
                }

                /// Consume the wrapper and return the complete JSON value.
                pub fn into_value(self) -> serde_json::Value {
                    return self.value;
                }

                #(#accessors)*
                #(#helpers)*
            }

            impl ::std::convert::TryFrom<serde_json::Value> for #name {
                type Error = ::std::string::String;

                fn try_from(value: serde_json::Value) -> ::std::result::Result<Self, Self::Error> {
                    let mut budget = #max_work;
                    let count = #count;
                    if budget == 0 {
                        return ::std::result::Result::Err("union validation limit exceeded".to_owned());
                    }
                    if count == 0 {
                        return ::std::result::Result::Err("anyOf requires at least one matching alternative".to_owned());
                    }
                    return ::std::result::Result::Ok(Self { value });
                }
            }

            #deserialize
        });
    }
    let derive = derive_attr(ModelDerives {
        serde: SerdeDerives {
            deserialize: false,
            ..derives.serde
        },
        ..derives
    });
    let untagged = if derives.serde.serialize {
        quote! { #[serde(untagged)] }
    } else {
        quote! {}
    };
    let deserialize = if derives.serde.deserialize {
        quote! {
            impl #name {
                #(#helpers)*
            }

            impl<'de> serde::Deserialize<'de> for #name {
                fn deserialize<__Deserializer: serde::Deserializer<'de>>(deserializer: __Deserializer) -> ::std::result::Result<Self, __Deserializer::Error> {
                    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
                    let mut budget = #max_work;
                    let matches = [#(#checks),*];
                    if budget == 0 {
                        return ::std::result::Result::Err(serde::de::Error::custom("union validation limit exceeded"));
                    }
                    if matches.iter().filter(|matched| **matched).count() != 1 {
                        return ::std::result::Result::Err(serde::de::Error::custom("oneOf requires exactly one matching alternative"));
                    }
                    return match matches.iter().position(|matched| *matched) {
                        ::std::option::Option::Some(index) => match index {
                            #(#decode)*
                            _ => ::std::result::Result::Err(serde::de::Error::custom("invalid oneOf alternative")),
                        },
                        ::std::option::Option::None => ::std::result::Result::Err(serde::de::Error::custom("oneOf has no matching alternative")),
                    };
                }
            }
        }
    } else {
        quote! {}
    };
    return Ok(quote! {
        #doc
        #derive
        #untagged
        #deprecated
        pub enum #name {
            #(#payloads)*
        }

        #deserialize
    });
}

fn helper(variant: usize, node: usize) -> proc_macro2::Ident {
    return format_ident!("__validate_{variant}_{node}");
}

fn decimal_multiple() -> TokenStream {
    return quote! {
        fn __decimal_parts(text: &str) -> ::std::option::Option<(u128, i32)> {
            let text = text.strip_prefix('-').unwrap_or(text);
            let (mantissa, mut exponent) = match text.split_once(['e', 'E']) {
                ::std::option::Option::Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
                ::std::option::Option::None => (text, 0),
            };
            if let ::std::option::Option::Some((_, fraction)) = mantissa.split_once('.') {
                exponent = exponent.checked_sub(<i32 as ::std::convert::TryFrom<usize>>::try_from(fraction.len()).ok()?)?;
            }
            let mut coefficient = 0_u128;
            for digit in mantissa.bytes().filter(|digit| *digit != b'.') {
                coefficient = coefficient.checked_mul(10)?.checked_add(u128::from(digit.checked_sub(b'0')?))?;
            }
            if coefficient == 0 {
                return ::std::option::Option::Some((0, 0));
            }
            while coefficient % 10 == 0 {
                coefficient /= 10;
                exponent = exponent.checked_add(1)?;
            }
            return ::std::option::Option::Some((coefficient, exponent));
        }

        fn __decimal_multiple(number: &str, bound: &str) -> bool {
            let ::std::option::Option::Some((coefficient, exponent)) = Self::__decimal_parts(number) else {
                return false;
            };
            let ::std::option::Option::Some((mut divisor, bound_exponent)) = Self::__decimal_parts(bound) else {
                return false;
            };
            if divisor == 0 {
                return false;
            }
            if coefficient == 0 {
                return true;
            }
            let shift = i64::from(exponent) - i64::from(bound_exponent);
            if shift < 0 {
                return false;
            }
            let (mut left, mut right) = (coefficient, divisor);
            while right != 0 {
                (left, right) = (right, left % right);
            }
            divisor /= left;
            // After reduction, only factors of 10 can cancel the remaining divisor.
            for factor in [2, 5] {
                let mut count = 0_i64;
                while divisor % factor == 0 {
                    divisor /= factor;
                    count += 1;
                }
                if count > shift {
                    return false;
                }
            }
            return divisor == 1;
        }
    };
}

fn schema_equality() -> TokenStream {
    let max_depth = MAX_VALIDATION_DEPTH;
    return quote! {
        fn __schema_equal(left: &serde_json::Value, right: &serde_json::Value, depth: usize, budget: &mut usize) -> bool {
            if depth > #max_depth || *budget <= 1 {
                *budget = 0;
                return false;
            }
            *budget -= 1;
            return match (left, right) {
                (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
                    let left_integer = left.as_i64().map(i128::from).or_else(|| left.as_u64().map(i128::from));
                    let right_integer = right.as_i64().map(i128::from).or_else(|| right.as_u64().map(i128::from));
                    match (left_integer, right_integer) {
                        (::std::option::Option::Some(left), ::std::option::Option::Some(right)) => left == right,
                        (::std::option::Option::Some(integer), ::std::option::Option::None) => right.as_f64().is_some_and(|number| number.fract() == 0.0 && number as i128 == integer),
                        (::std::option::Option::None, ::std::option::Option::Some(integer)) => left.as_f64().is_some_and(|number| number.fract() == 0.0 && number as i128 == integer),
                        (::std::option::Option::None, ::std::option::Option::None) => left.as_f64() == right.as_f64(),
                    }
                }
                (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
                    left.len() == right.len() && left.iter().zip(right).all(|(left, right)| Self::__schema_equal(left, right, depth + 1, budget))
                }
                (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
                    left.len() == right.len() && left.iter().all(|(key, left)| right.get(key).is_some_and(|right| Self::__schema_equal(left, right, depth + 1, budget)))
                }
                _ => left == right,
            };
        }
    };
}

fn integer_value(value: &Value) -> Option<i128> {
    return value
        .as_i64()
        .map(i128::from)
        .or_else(|| return value.as_u64().map(i128::from))
        .or_else(|| {
            let number = value.as_f64()?;
            if number.fract() != 0.0_f64 {
                return None;
            }
            return format!("{number:.0}").parse().ok();
        });
}

fn integer_order(bound: i128) -> TokenStream {
    return quote! {
        if let ::std::option::Option::Some(number) = value.as_i64() {
            ::std::option::Option::Some(i128::from(number).cmp(&#bound))
        } else if let ::std::option::Option::Some(number) = value.as_u64() {
            ::std::option::Option::Some(i128::from(number).cmp(&#bound))
        } else {
            value.as_f64().map(|number| {
                let integer_order = (number as i128).cmp(&#bound);
                return if integer_order == ::std::cmp::Ordering::Equal {
                    number.fract().partial_cmp(&0.0).unwrap_or(::std::cmp::Ordering::Equal)
                } else {
                    integer_order
                };
            })
        }
    };
}

fn predicate(variant: usize, node: &UnionValidationNode) -> TokenStream {
    let keys = &node.keywords;
    let mut checks = Vec::new();
    let nullable = keys.get("nullable").and_then(Value::as_bool) == Some(true);
    match keys.get("type").and_then(Value::as_str) {
        Some("string") => checks.push(quote! { value.is_string() }),
        Some("integer") => checks.push(quote! { value.as_f64().is_some_and(|number| number.fract() == 0.0) }),
        Some("number") => checks.push(quote! { value.is_number() }),
        Some("boolean") => checks.push(quote! { value.is_boolean() }),
        Some("array") => checks.push(quote! { value.is_array() }),
        Some("object") => checks.push(quote! { value.is_object() }),
        _ => {}
    }
    if nullable {
        for check in &mut checks {
            *check = quote! { value.is_null() || (#check) };
        }
    }
    if keys.get("type").and_then(Value::as_str) == Some("string") {
        match keys.get("format").and_then(Value::as_str) {
            Some("date") => checks.push(quote! { value.as_str().is_none_or(|text| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()) }),
            Some("date-time") => checks.push(quote! { value.as_str().is_none_or(|text| chrono::DateTime::parse_from_rfc3339(text).is_ok()) }),
            Some("uuid") => checks.push(quote! { value.as_str().is_none_or(|text| uuid::Uuid::parse_str(text).is_ok()) }),
            _ => {}
        }
    }
    if let Some(pattern) = keys.get("pattern").and_then(Value::as_str) {
        checks.push(quote! {
            value.as_str().is_none_or(|text| {
                static PATTERN: ::std::sync::OnceLock<::std::result::Result<regex::Regex, regex::Error>> = ::std::sync::OnceLock::new();
                return PATTERN.get_or_init(|| regex::Regex::new(#pattern)).as_ref().is_ok_and(|pattern| pattern.is_match(text));
            })
        });
    }
    if let Some(values) = keys.get("enum").and_then(Value::as_array) {
        let values = values.iter().map(|value| {
            let text = value.to_string();
            return quote! {
                serde_json::from_str::<serde_json::Value>(#text).is_ok_and(|expected| {
                    return Self::__schema_equal(value, &expected, depth + 1, budget);
                })
            };
        });
        checks.push(quote! { false #(|| #values)* });
    }
    for (key, lower) in [("minimum", true), ("maximum", false)] {
        if let Some(bound) = keys.get(key).and_then(Value::as_f64) {
            let exclusive = keys
                .get(if lower { "exclusiveMinimum" } else { "exclusiveMaximum" })
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if let Some(integer) = keys.get(key).and_then(integer_value) {
                let order = integer_order(integer);
                let compare = match (lower, exclusive) {
                    (true, true) => quote! { order.is_gt() },
                    (true, false) => quote! { !order.is_lt() },
                    (false, true) => quote! { order.is_lt() },
                    (false, false) => quote! { !order.is_gt() },
                };
                checks.push(quote! { (#order).is_none_or(|order| #compare) });
                continue;
            }
            let compare = match (lower, exclusive) {
                (true, true) => quote! { number > #bound },
                (true, false) => quote! { number >= #bound },
                (false, true) => quote! { number < #bound },
                (false, false) => quote! { number <= #bound },
            };
            checks.push(quote! { value.as_f64().is_none_or(|number| #compare) });
        }
    }
    if let Some(Value::Number(bound)) = keys.get("multipleOf") {
        let bound = bound.to_string();
        checks.push(quote! {
            match value {
                serde_json::Value::Number(number) => Self::__decimal_multiple(&number.to_string(), #bound),
                _ => true,
            }
        });
    }
    for (min, max, size) in [
        (
            "minLength",
            "maxLength",
            quote! { value.as_str().map(|text| text.chars().count()) },
        ),
        (
            "minItems",
            "maxItems",
            quote! { value.as_array().map(::std::vec::Vec::len) },
        ),
        (
            "minProperties",
            "maxProperties",
            quote! { value.as_object().map(serde_json::Map::len) },
        ),
    ] {
        if let Some(limit) = keys.get(min).and_then(Value::as_u64) {
            let limit = usize::try_from(limit).unwrap_or(usize::MAX);
            checks.push(quote! { (#size).is_none_or(|length| length >= #limit) });
        }
        if let Some(limit) = keys.get(max).and_then(Value::as_u64) {
            let limit = usize::try_from(limit).unwrap_or(usize::MAX);
            checks.push(quote! { (#size).is_none_or(|length| length <= #limit) });
        }
    }
    if keys.get("uniqueItems").and_then(Value::as_bool) == Some(true) {
        checks.push(quote! { value.as_array().is_none_or(|items| items.iter().enumerate().all(|(index, item)| !items[..index].iter().any(|other| *budget == 0 || Self::__schema_equal(item, other, depth + 1, budget)))) });
    }
    let mut object_checks = Vec::new();
    if let Some(required) = keys.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            object_checks.push(quote! { object.contains_key(#name) });
        }
    }
    for (name, child) in &node.properties {
        let method = helper(variant, *child);
        object_checks.push(quote! { object.get(#name).is_none_or(|value| Self::#method(value, depth + 1, budget)) });
    }
    if keys.get("additionalProperties") == Some(&Value::Bool(false)) || node.additional.is_some() {
        let names: Vec<_> = node.properties.iter().map(|(name, _)| return name).collect();
        let unknown = match node.additional {
            Some(child) => {
                let method = helper(variant, child);
                quote! { Self::#method(value, depth + 1, budget) }
            }
            None => quote! { false },
        };
        object_checks.push(quote! { object.iter().all(|(key, value)| [#(#names),*].contains(&key.as_str()) || { let _ = value; #unknown }) });
    }
    if !object_checks.is_empty() {
        checks.push(quote! { value.as_object().is_none_or(|object| true #(&& (#object_checks))*) });
    }
    if let Some(child) = node.items {
        let method = helper(variant, child);
        checks.push(
            quote! { value.as_array().is_none_or(|items| items.iter().all(|value| Self::#method(value, depth + 1, budget))) },
        );
    }
    let children: Vec<_> = node
        .children
        .iter()
        .map(|child| {
            let method = helper(variant, *child);
            return quote! { Self::#method(value, depth + 1, budget) };
        })
        .collect();
    match (
        keys.contains_key("oneOf"),
        keys.contains_key("anyOf"),
        keys.contains_key("allOf"),
    ) {
        (true, _, _) => checks.push(quote! { [#(#children),*].into_iter().filter(|matched| *matched).count() == 1 }),
        (false, true, _) => checks.push(quote! { false #(|| #children)* }),
        (false, false, true) => checks.push(quote! { true #(&& #children)* }),
        (false, false, false) => {}
    }
    return quote! { true #(&& (#checks))* };
}
