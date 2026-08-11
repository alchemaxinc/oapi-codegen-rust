//! Readers for the `x-` extensions the generator understands.
//!
//! A reader tells an absent key apart from a key that carries the wrong kind of
//! value. An absent key gives `None`, and the generator falls back to its
//! default. A wrong value ends the run, because the author wrote the key to
//! change something and a silent fallback would hide that nothing changed.

use indexmap::IndexMap;
use serde_json::Value;

use crate::error::Error;
use crate::error::Result;

/// The extension map an OpenAPI object carries.
pub(crate) type Extensions = IndexMap<String, Value>;

/// Read a string-valued extension, for example `x-rust-name`.
///
/// `at` names the place the key sits, so the message can point at it.
pub(crate) fn str_value<'a>(extensions: &'a Extensions, key: &str, at: &str) -> Result<Option<&'a str>> {
    let Some(value) = extensions.get(key) else {
        return Ok(None);
    };
    let Some(text) = value.as_str() else {
        return Err(wrong_value(key, at, "a string", value));
    };
    return Ok(Some(text));
}

/// Read a boolean-valued extension, for example `x-omitempty`.
pub(crate) fn bool_value(extensions: &Extensions, key: &str, at: &str) -> Result<Option<bool>> {
    let Some(value) = extensions.get(key) else {
        return Ok(None);
    };
    let Some(flag) = value.as_bool() else {
        return Err(wrong_value(key, at, "`true` or `false`", value));
    };
    return Ok(Some(flag));
}

/// Read an integer-valued extension, for example `x-order`.
pub(crate) fn i64_value(extensions: &Extensions, key: &str, at: &str) -> Result<Option<i64>> {
    let Some(value) = extensions.get(key) else {
        return Ok(None);
    };
    let Some(number) = value.as_i64() else {
        return Err(wrong_value(key, at, "a whole number", value));
    };
    return Ok(Some(number));
}

/// Read a string-list extension, for example `x-enum-varnames`.
pub(crate) fn str_list_value<'a>(extensions: &'a Extensions, key: &str, at: &str) -> Result<Option<Vec<&'a str>>> {
    let Some(value) = extensions.get(key) else {
        return Ok(None);
    };
    let Some(items) = value.as_array() else {
        return Err(wrong_value(key, at, "a list of strings", value));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(text) = item.as_str() else {
            return Err(wrong_value(key, at, "a list in which every entry is a string", item));
        };
        out.push(text);
    }
    return Ok(Some(out));
}

/// Build the error a wrong value gives.
fn wrong_value(key: &str, at: &str, expected: &str, found: &Value) -> Error {
    return Error::InvalidExtensionValue {
        key: key.to_owned(),
        at: at.to_owned(),
        expected: expected.to_owned(),
        found: type_name(found).to_owned(),
    };
}

/// The name of the kind of value a document wrote, for the message.
fn type_name(value: &Value) -> &'static str {
    return match value {
        Value::Null => "null",
        Value::Bool(_) => "`true` or `false`",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "a map",
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an extension map from pairs of key and JSON text.
    fn map(pairs: &[(&str, &str)]) -> Extensions {
        return pairs
            .iter()
            .map(|(key, json)| {
                let value = serde_json::from_str(json).expect("the test writes valid JSON");
                return ((*key).to_owned(), value);
            })
            .collect();
    }

    #[test]
    fn an_absent_key_gives_none() {
        let extensions = map(&[]);
        assert_eq!(
            str_value(&extensions, "x-rust-name", "Thing").expect("absent is not a fault"),
            None
        );
        assert_eq!(
            bool_value(&extensions, "x-omitempty", "Thing").expect("absent is not a fault"),
            None
        );
        assert_eq!(
            i64_value(&extensions, "x-order", "Thing").expect("absent is not a fault"),
            None
        );
        assert_eq!(
            str_list_value(&extensions, "x-enum-varnames", "Thing").expect("absent is not a fault"),
            None
        );
    }

    #[test]
    fn a_right_value_reaches_the_caller() {
        let extensions = map(&[
            ("x-rust-name", r#""Widget""#),
            ("x-omitempty", "true"),
            ("x-order", "3"),
            ("x-enum-varnames", r#"["A", "B"]"#),
        ]);
        assert_eq!(
            str_value(&extensions, "x-rust-name", "Thing").expect("a string is right"),
            Some("Widget")
        );
        assert_eq!(
            bool_value(&extensions, "x-omitempty", "Thing").expect("a boolean is right"),
            Some(true)
        );
        assert_eq!(
            i64_value(&extensions, "x-order", "Thing").expect("a number is right"),
            Some(3)
        );
        assert_eq!(
            str_list_value(&extensions, "x-enum-varnames", "Thing").expect("a list of strings is right"),
            Some(vec!["A", "B"])
        );
    }

    /// Every reader rejects a value of the wrong kind, and names both the key
    /// and the place it sits.
    #[test]
    fn a_wrong_value_ends_the_run() {
        /// Read one key with one reader and give back only the outcome.
        type Read = fn(&Extensions) -> Result<()>;

        let read_str: Read = |extensions| {
            str_value(extensions, "x-rust-name", "Thing")?;
            return Ok(());
        };
        let read_bool: Read = |extensions| {
            bool_value(extensions, "x-omitempty", "Thing")?;
            return Ok(());
        };
        let read_i64: Read = |extensions| {
            i64_value(extensions, "x-order", "Thing")?;
            return Ok(());
        };
        let read_list: Read = |extensions| {
            str_list_value(extensions, "x-enum-varnames", "Thing")?;
            return Ok(());
        };

        let cases: [(&str, &str, Read); 5] = [
            ("x-rust-name", "123", read_str),
            ("x-omitempty", r#""yes""#, read_bool),
            ("x-order", r#""first""#, read_i64),
            ("x-enum-varnames", r#""A,B""#, read_list),
            ("x-enum-varnames", "[1, 2]", read_list),
        ];
        for (key, json, read) in cases {
            let extensions = map(&[(key, json)]);
            let error = read(&extensions).expect_err("a wrong value is a fault");
            let text = error.to_string();
            assert!(
                text.contains(key),
                "`{key}` with `{json}`: the message hides the key: {text}"
            );
            assert!(
                text.contains("Thing"),
                "`{key}` with `{json}`: the message hides the place: {text}"
            );
        }
    }
}
