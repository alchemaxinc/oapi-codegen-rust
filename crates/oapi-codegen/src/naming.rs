//! Conversion of OpenAPI names into valid, idiomatic Rust identifiers and
//! derivation of per-operation artifact names.

use proc_macro2::Ident;
use proc_macro2::Span;

/// `snake_case` / `UpperCamelCase` conversion used by [`to_ident`].
mod casing;

/// Derivation of the Rust names of the artifacts generated for an operation.
pub mod operations;

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
    /// `SCREAMING_SNAKE_CASE` — used for `const`/`static` items.
    ScreamingSnake,
}

/// Convert an arbitrary OpenAPI name into a valid Rust identifier in the
/// requested case, escaping keywords and leading digits.
pub fn to_ident(name: &str, case: Case) -> RustIdent {
    let cased = match case {
        Case::Pascal => casing::to_upper_camel_case(name),
        Case::Snake => casing::to_snake_case(name),
        Case::ScreamingSnake => casing::to_screaming_snake_case(name),
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

/// Ensure `ident` is unique among the identifiers already recorded in `seen`,
/// appending the lowest free numeric suffix (`Foo`, `Foo2`, `Foo3`, ...) on
/// collision. The chosen identifier is inserted into `seen`, which is keyed by
/// logical identifier text (so `foo` and `Foo` are treated as distinct only
/// when their cased forms differ).
///
/// Used to keep generated type names and enum variants unique when distinct
/// OpenAPI names collapse onto the same Rust identifier (e.g. `foo-bar` and
/// `fooBar` both becoming `FooBar`).
pub fn deconflict_ident(ident: RustIdent, seen: &mut std::collections::HashSet<String>) -> RustIdent {
    if seen.insert(ident.logical().to_owned()) {
        return ident;
    }
    let mut suffix = 2;
    loop {
        let candidate = to_ident(&format!("{} {suffix}", ident.logical()), Case::Pascal);
        if seen.insert(candidate.logical().to_owned()) {
            return candidate;
        }
        suffix += 1;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case_types() {
        let cases = [
            ("ErrorResponse", "ErrorResponse", false),
            ("payment_form", "PaymentForm", false),
            ("da", "Da", false),
            ("PET_SHOP_SIGNUP_REQUEST", "PetShopSignupRequest", false),
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
    fn deconflict_ident_suffixes_collisions() {
        let mut seen = std::collections::HashSet::new();
        // Distinct OpenAPI names that collapse onto the same identifier gain the
        // lowest free numeric suffix, in the order they are seen.
        let first = deconflict_ident(to_ident("order-item", Case::Pascal), &mut seen);
        let second = deconflict_ident(to_ident("orderItem", Case::Pascal), &mut seen);
        let third = deconflict_ident(to_ident("Order_Item", Case::Pascal), &mut seen);
        assert_eq!(first.logical(), "OrderItem");
        assert_eq!(second.logical(), "OrderItem2");
        assert_eq!(third.logical(), "OrderItem3");
        // A distinct identifier is left untouched.
        let other = deconflict_ident(to_ident("cart", Case::Pascal), &mut seen);
        assert_eq!(other.logical(), "Cart");
    }

    #[test]
    fn casing_word_boundaries() {
        // Representative word-boundary cases: camelCase, acronyms, SHOUTY, and mixed separators.
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
