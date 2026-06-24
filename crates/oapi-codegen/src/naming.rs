//! Conversion of OpenAPI names into valid, idiomatic Rust identifiers.

use proc_macro2::Ident;
use proc_macro2::Span;

/// A Rust identifier together with whether it must be emitted as a raw
/// identifier (`r#name`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustIdent {
    /// The logical identifier text (without any `r#` prefix).
    text: String,
    /// Whether the identifier must be emitted in raw form.
    raw: bool,
}

impl RustIdent {
    /// The logical name as serde would serialize it (no `r#`).
    pub fn logical(&self) -> &str {
        return &self.text;
    }

    /// Build a [`proc_macro2::Ident`] for use in `quote!`.
    pub fn to_token(&self) -> Ident {
        let ident = if self.raw {
            Ident::new_raw(&self.text, Span::call_site())
        } else {
            Ident::new(&self.text, Span::call_site())
        };
        return ident;
    }
}

/// Casing to apply when converting a name.
#[derive(Debug, Clone, Copy)]
pub enum Case {
    /// `PascalCase` — used for types and enum variants.
    Pascal,
    /// `snake_case` — used for struct fields.
    Snake,
}

/// Convert an arbitrary OpenAPI name into a valid Rust identifier in the
/// requested case, escaping keywords and leading digits.
pub fn to_ident(name: &str, case: Case) -> RustIdent {
    let cased = match case {
        Case::Pascal => casing::to_upper_camel_case(name),
        Case::Snake => casing::to_snake_case(name),
    };

    let cased = if cased.is_empty() { "Unnamed".to_owned() } else { cased };

    // An identifier may not start with a digit.
    let starts_with_digit = cased.chars().next().map(char::is_numeric).unwrap_or(false);
    let cased = if starts_with_digit { format!("_{cased}") } else { cased };

    match classify_ident(&cased) {
        IdentForm::Plain => {
            return RustIdent {
                text: cased,
                raw: false,
            };
        }
        IdentForm::Raw => {
            return RustIdent { text: cased, raw: true };
        }
        IdentForm::Suffix => {
            return RustIdent {
                text: format!("{cased}_"),
                raw: false,
            };
        }
    }
}

/// Compute the serde `rename` value for a member, given its wire name and the
/// chosen Rust identifier. Returns `None` when no rename attribute is needed.
pub fn rename_for(wire: &str, ident: &RustIdent) -> Option<String> {
    if ident.logical() == wire {
        return None;
    }
    return Some(wire.to_owned());
}

/// How a candidate identifier string must be emitted to be valid Rust.
enum IdentForm {
    /// Usable verbatim.
    Plain,
    /// A keyword that must be written as a raw identifier (`r#name`).
    Raw,
    /// A keyword that cannot be raw and must be escaped with a trailing `_`.
    Suffix,
}

/// Classify `s` using `syn`'s identifier grammar so the keyword set stays in
/// sync with the compiler as `syn` is updated — there is no runtime reflection
/// for Rust keywords, but `syn` already encodes the grammar we depend on.
///
/// `syn` cannot know the target edition, so edition-2024 reserved words that it
/// still accepts as plain identifiers are escaped explicitly via
/// [`is_edition_2024_keyword`].
fn classify_ident(s: &str) -> IdentForm {
    if syn::parse_str::<syn::Ident>(s).is_ok() && !is_edition_2024_keyword(s) {
        return IdentForm::Plain;
    }
    if syn::parse_str::<syn::Ident>(&format!("r#{s}")).is_ok() {
        return IdentForm::Raw;
    }
    return IdentForm::Suffix;
}

/// Reserved words introduced by newer editions that `syn`'s edition-agnostic
/// parser still accepts as plain identifiers. They are raw-escaped so generated
/// code compiles under edition 2024 and later.
fn is_edition_2024_keyword(s: &str) -> bool {
    return matches!(s, "gen");
}

/// Minimal, self-contained reimplementation of the subset of the `heck` crate
/// that we use: `snake_case` and `UpperCamelCase`. The word-boundary algorithm
/// matches `heck` exactly (an uppercase run is one word, except its last letter
/// joins a following lowercase run; non-alphanumerics are boundaries).
mod casing {
    /// Convert `s` to `snake_case`.
    pub fn to_snake_case(s: &str) -> String {
        let mut out = String::new();
        transform(s, lowercase, push_underscore, &mut out);
        return out;
    }

    /// Convert `s` to `UpperCamelCase` (a.k.a. `PascalCase`).
    pub fn to_upper_camel_case(s: &str) -> String {
        let mut out = String::new();
        transform(s, capitalize, no_boundary, &mut out);
        return out;
    }

    /// Case of the last cased character seen in the current word.
    #[derive(Clone, Copy, PartialEq)]
    enum WordMode {
        /// No cased character seen since the last boundary.
        Boundary,
        /// The previous cased character was lowercase.
        Lowercase,
        /// The previous cased character was uppercase.
        Uppercase,
    }

    /// Split `s` into words and append each, separated by `boundary`, after
    /// passing it through `with_word`.
    fn transform(s: &str, with_word: fn(&str, &mut String), boundary: fn(&mut String), out: &mut String) {
        let mut first_word = true;

        // Append one sub-word, prefixing the boundary for every word but the first.
        let mut emit = |slice: &str, out: &mut String| {
            if !first_word {
                boundary(out);
            }
            first_word = false;
            with_word(slice, out);
        };

        for word in s.split(|c: char| {
            return !c.is_alphanumeric();
        }) {
            let mut char_indices = word.char_indices().peekable();
            let mut init = 0;
            let mut mode = WordMode::Boundary;

            while let Some((i, c)) = char_indices.next() {
                let Some(&(next_i, next)) = char_indices.peek() else {
                    // Last character of the word: flush the trailing slice.
                    emit(&word[init..], out);
                    break;
                };

                let next_mode = if c.is_lowercase() {
                    WordMode::Lowercase
                } else if c.is_uppercase() {
                    WordMode::Uppercase
                } else {
                    mode
                };

                if next_mode == WordMode::Lowercase && next.is_uppercase() {
                    // lower→Upper boundary: split before the uppercase (`fooBar` → `foo`|`Bar`).
                    emit(&word[init..next_i], out);
                    init = next_i;
                    mode = WordMode::Boundary;
                } else if mode == WordMode::Uppercase && c.is_uppercase() && next.is_lowercase() {
                    // End of an uppercase run: the trailing uppercase starts the next word (`ABc` → `A`|`Bc`).
                    emit(&word[init..i], out);
                    init = i;
                    mode = WordMode::Boundary;
                } else {
                    mode = next_mode;
                }
            }
        }
    }

    /// Append `s` lowercased, mirroring `heck`'s final-sigma handling.
    fn lowercase(s: &str, out: &mut String) {
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == 'Σ' && chars.peek().is_none() {
                out.push('ς');
            } else {
                for lower in c.to_lowercase() {
                    out.push(lower);
                }
            }
        }
    }

    /// Append `s` with its first character uppercased and the rest lowercased.
    fn capitalize(s: &str, out: &mut String) {
        let mut char_indices = s.char_indices();
        if let Some((_, c)) = char_indices.next() {
            for upper in c.to_uppercase() {
                out.push(upper);
            }
            if let Some((i, _)) = char_indices.next() {
                lowercase(&s[i..], out);
            }
        }
    }

    /// Word separator for `snake_case`.
    fn push_underscore(out: &mut String) {
        out.push('_');
    }

    /// Word separator for `UpperCamelCase` (none).
    fn no_boundary(_out: &mut String) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case_types() {
        let cases = [
            ("ErrorResponse", "ErrorResponse", false),
            ("payment_form", "PaymentForm", false),
            ("da", "Da", false),
            ("MACHSIKE_SIGNUP_REQUEST", "MachsikeSignupRequest", false),
        ];
        for (input, expected, raw) in cases {
            let ident = to_ident(input, Case::Pascal);
            assert_eq!(ident.logical(), expected, "input {input}");
            assert_eq!(ident.raw, raw, "input {input}");
        }
    }

    #[test]
    fn snake_case_fields_and_keywords() {
        let ty = to_ident("type", Case::Snake);
        assert_eq!(ty.logical(), "type");
        assert!(ty.raw, "`type` should be a raw identifier");
        assert_eq!(rename_for("type", &ty), None);

        let email = to_ident("customer_email", Case::Snake);
        assert_eq!(email.logical(), "customer_email");
        assert!(!email.raw);
        assert_eq!(rename_for("customer_email", &email), None);
    }

    #[test]
    fn rename_when_casing_differs() {
        let ident = to_ident("da", Case::Pascal);
        assert_eq!(rename_for("da", &ident), Some("da".to_owned()));
    }

    #[test]
    fn keyword_that_cannot_be_raw_is_suffixed() {
        let ident = to_ident("self", Case::Snake);
        assert_eq!(ident.logical(), "self_");
        assert!(!ident.raw);
    }

    #[test]
    fn edition_2024_keyword_is_raw() {
        // `gen` is reserved in edition 2024 but `syn`'s edition-agnostic parser
        // accepts it as a plain identifier, so we escape it ourselves.
        let ident = to_ident("gen", Case::Snake);
        assert_eq!(ident.logical(), "gen");
        assert!(ident.raw, "`gen` should be a raw identifier under edition 2024");
    }

    #[test]
    fn casing_matches_heck() {
        // Parity vectors lifted from heck's own test suite.
        let snake = [
            ("CamelCase", "camel_case"),
            ("XMLHttpRequest", "xml_http_request"),
            ("FIELD_NAME11", "field_name11"),
            (
                "this-contains_ ALLKinds OfWord_Boundaries",
                "this_contains_all_kinds_of_word_boundaries",
            ),
        ];
        for (input, expected) in snake {
            assert_eq!(casing::to_snake_case(input), expected, "snake {input}");
        }

        let pascal = [
            ("CamelCase", "CamelCase"),
            ("XMLHttpRequest", "XmlHttpRequest"),
            ("SHOUTY_SNAKE_CASE", "ShoutySnakeCase"),
            (
                "this-contains_ ALLKinds OfWord_Boundaries",
                "ThisContainsAllKindsOfWordBoundaries",
            ),
        ];
        for (input, expected) in pascal {
            assert_eq!(casing::to_upper_camel_case(input), expected, "pascal {input}");
        }
    }
}
