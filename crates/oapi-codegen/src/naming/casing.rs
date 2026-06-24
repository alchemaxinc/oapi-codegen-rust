//! Self-contained `snake_case` and `UpperCamelCase` conversion.
//!
//! Word-boundary rules: non-alphanumeric characters separate words, and an
//! uppercase run forms a single word except that its final letter joins a
//! following lowercase run (so `XMLHttp` splits into `XML` + `Http`).

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

/// Append `s` lowercased, mapping a word-final capital sigma `Σ` to its final
/// form `ς` (which `char::to_lowercase` does not).
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
