//! Terminal console output for the CLI: guided, colorized error reporting.
//!
//! Everything here writes to stderr through [`anstream`], which automatically
//! strips ANSI styling when stderr is not a terminal or `NO_COLOR` is set, so
//! the output degrades gracefully in pipes and logs. Styling is applied with
//! [`owo_colors`]. The goal is a "guided" experience: every failure states
//! what went wrong and, where possible, a concrete next step.

use std::io::ErrorKind;
use std::path::Path;

use anstream::eprint;
use anstream::eprintln;
use oapi_codegen::Error;
use oapi_codegen::config::Generate;
use oapi_codegen::deps::Dependency;
use owo_colors::OwoColorize;

/// Example `generate:` block shown when a config enables no artifacts. Each
/// line is printed dimmed and indented under the hint.
const GENERATE_EXAMPLE: &str = "\
generate:
  models: true            # structs/enums from components.schemas
  std-http-server: true   # an axum server trait from the paths
  client: true            # a blocking reqwest client from the paths
  server-urls: true       # constants/builders from the top-level `servers:` block";

/// A lightweight summary of a spec, used to explain empty output.
#[derive(Debug, Clone, Copy)]
pub struct SpecStats {
    /// Number of `components.schemas` entries (after filtering).
    pub schemas: usize,
    /// Number of paths declared in the document (after filtering).
    pub paths: usize,
    /// Number of top-level `servers:` entries declared in the document.
    pub servers: usize,
}

/// Return `true` when generated `code` contains no items, only the header and
/// blank lines. Such a file is confusing to write to disk, so the CLI treats it
/// as a failure and explains why instead.
///
/// The header is stripped as the one string the emitter writes, and not matched
/// by shape. It holds an inner attribute as well as the do-not-edit comment, and
/// a shape test that accepted any `#!` line would accept a real attribute too.
pub fn is_effectively_empty(code: &str) -> bool {
    let body = code.strip_prefix(oapi_codegen::emit::HEADER).unwrap_or(code);
    return body.lines().all(|line| {
        let trimmed = line.trim();
        return trimmed.is_empty() || trimmed.starts_with("//");
    });
}

/// Print a guided, styled error report for a generator `err` to stderr.
///
/// An [`Error::Validation`] holds several independent problems. Each one gets its
/// own numbered heading and its own hints, because one message with every hint
/// after it does not show which hint corrects which problem.
pub fn report_error(err: &Error) {
    if let Error::Validation { problems } = err {
        eprintln!(
            "{} found {} problems in the spec.",
            "error:".red().bold(),
            problems.len().bold()
        );
        for (index, problem) in problems.iter().enumerate() {
            // 1-based, to match how the count above reads to a person.
            let position = index.saturating_add(1);
            eprintln!("  {} {}", format!("{position}.").red().bold(), problem.bold());
            for hint in hints_for(problem) {
                eprintln!("     {} {hint}", "hint:".cyan().bold());
            }
        }
        return;
    }
    eprintln!("{} {}", "error:".red().bold(), err.bold());
    for hint in hints_for(err) {
        eprintln!("  {} {hint}", "hint:".cyan().bold());
    }
}

/// Report that no output destination was given, and show how to fix it.
pub fn report_no_output() {
    eprintln!("{} no output destination was given.", "error:".red().bold());
    eprintln!(
        "  {} pass `--output-file <file>` on the command line, or set `output:` in your config.",
        "hint:".cyan().bold()
    );
}

/// Report that a loaded config enables no artifacts, and show how to fix it.
pub fn report_no_artifacts(config: &Path) {
    eprintln!(
        "{} the config `{}` enables nothing to generate.",
        "error:".red().bold(),
        config.display()
    );
    eprintln!(
        "  {} enable at least one artifact under `generate:`",
        "hint:".cyan().bold()
    );
    print_snippet(GENERATE_EXAMPLE);
}

/// Print a multi-line code snippet dimmed and indented under a hint.
fn print_snippet(snippet: &str) {
    for line in snippet.lines() {
        eprintln!("      {}", line.dimmed());
    }
}

/// Report that generation succeeded but produced no code, explaining the
/// mismatch between what the config asked for and what the spec contains.
pub fn report_empty_output(spec: &Path, stats: &SpecStats, generate: &Generate) {
    eprintln!(
        "{} generation of `{}` produced no code.",
        "error:".red().bold(),
        spec.display()
    );
    eprintln!(
        "  the spec declares {} and {}.",
        count("schema", stats.schemas).bold(),
        count("path", stats.paths).bold()
    );
    for hint in empty_output_hints(stats, generate) {
        eprintln!("  {} {hint}", "hint:".cyan().bold());
    }
}

/// Report a successful write to `path`.
pub fn report_wrote(path: &Path) {
    eprintln!("{} wrote {}", "✓".green().bold(), path.display());
}

/// Report that `--check` found `path` up to date.
pub fn report_check_passed(path: &Path) {
    eprintln!("{} {} is up to date", "✓".green().bold(), path.display());
}

/// Report that `--check` found drift, and name the command that resolves it.
///
/// The message states which of the two cases holds, because an absent file and a
/// stale file need the reader to look at different things. Both have one remedy,
/// which is a run with no `--check`.
pub fn report_drift(path: &Path, absent: bool) {
    if absent {
        eprintln!(
            "{} {} does not exist.",
            "error:".red().bold(),
            path.display().to_string().bold()
        );
    } else {
        eprintln!(
            "{} {} is out of date with the spec.",
            "error:".red().bold(),
            path.display().to_string().bold()
        );
    }
    eprintln!(
        "  {} run the same command without `--check` to update it, and commit the result.",
        "hint:".cyan().bold()
    );
}

/// After a successful write, list the external crates the generated code
/// references so the consumer can add them to `Cargo.toml` — Cargo does not
/// infer them from `use` paths the way `go mod tidy` does. Prints nothing when
/// the output references no external crates (e.g. `server-urls` only).
pub fn report_dependencies(deps: &[Dependency]) {
    if deps.is_empty() {
        return;
    }
    eprintln!(
        "  {} add the crates the generated code references to Cargo.toml:",
        "note:".cyan().bold()
    );
    for dep in deps {
        eprintln!("      {}", dep.toml().dimmed());
    }
    eprintln!("      {}", "# or:".dimmed());
    for dep in deps {
        eprintln!("      {}", dep.cargo_add().dimmed());
    }
}

/// Ask whether to run the `cargo add` commands now. Returns `false` on EOF or a
/// non-affirmative answer. Only meaningful on an interactive terminal.
pub fn prompt_install_dependencies() -> bool {
    use std::io::Write;
    eprint!("  {} run these `cargo add` commands now? [y/N] ", "?".cyan().bold());
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    let answer = answer.trim().to_ascii_lowercase();
    return answer == "y" || answer == "yes";
}

/// Report that a dependency is being added via `cargo add`.
pub fn report_installing(dep: &Dependency) {
    eprintln!("  {} {}", "+".green().bold(), dep.cargo_add().dimmed());
}

/// Report that a `cargo add` invocation failed, without aborting — the output
/// file is already written, so a failed convenience step is a warning, not a
/// fatal error.
pub fn report_install_failed(dep: &Dependency, detail: &str) {
    eprintln!(
        "  {} `{}` failed: {detail}",
        "warning:".yellow().bold(),
        dep.cargo_add()
    );
}

/// Build the context-specific hints shown after an error message.
fn hints_for(err: &Error) -> Vec<String> {
    match err {
        Error::ReadSpec { path, source } => {
            return io_read_hints("spec file", path, source.kind());
        }
        Error::ReadConfig { path, source } => {
            return io_read_hints("config file", path, source.kind());
        }
        Error::ReadRefFile { file, source } => {
            return io_read_hints("referenced file", file, source.kind());
        }
        Error::ParseSpec { source, .. } => {
            return parse_spec_hints(&source.to_string());
        }
        Error::ParseRefFile { .. } => {
            return vec!["The referenced file must be a valid OpenAPI 3 fragment (YAML or JSON).".to_owned()];
        }
        Error::ParseConfig { .. } => {
            return config_hints();
        }
        Error::WriteOutput { path, .. } => {
            return vec![format!("Check that the directory for `{path}` is writable.")];
        }
        // An absent file is drift and not this error, so the reader has a path
        // that exists and that the process cannot read.
        Error::ReadOutput { path, .. } => {
            return vec![format!("Check that `{path}` is a readable file and not a directory.")];
        }
        Error::Unimplemented(_) => {
            return vec!["This generation mode is not supported yet.".to_owned()];
        }
        Error::UnresolvedRef(_) => {
            return vec![
                "The document has no component at that pointer.".to_owned(),
                "Check the spelling, and check that the component exists.".to_owned(),
                "For a cross-file ref, the component must exist in the other file.".to_owned(),
            ];
        }
        Error::UnsupportedRef { .. } => {
            return vec![
                "A same-document `#/components/...` pointer is the supported form at any site.".to_owned(),
                "A cross-file `<file>#/components/...` ref resolves at a response, a parameter, or a request body. Give the file an `import-mapping` entry. The path is relative to the spec.".to_owned(),
                "Inside a schema, only a same-document ref resolves. This covers a property, `items`, `additionalProperties`, `allOf`, and a union member.".to_owned(),
            ];
        }
        Error::UnsupportedSchema { reason, .. } if reason.contains("does not reach the type") => {
            if reason.contains("Properties") {
                return vec![
                    "`minProperties` and `maxProperties` read a free-form map. A schema that names its properties fixes the count already.".to_owned(),
                ];
            }
            if reason.contains("uniqueItems") {
                return vec![
                    "`uniqueItems` reads a list of numbers, strings, or booleans. A list of models cannot always compare.".to_owned(),
                ];
            }
            return vec![
                "A `format` can name a type that is no longer a string, such as a date or a UUID. Drop the `format` to keep the string and the rule.".to_owned(),
                "An `x-rust-type` does the same. The rules of that type are its own.".to_owned(),
            ];
        }
        Error::UnsupportedSchema { reason, .. } if reason.contains("is not above zero") => {
            return vec!["JSON Schema asks for a `multipleOf` above zero. A step of zero divides by zero.".to_owned()];
        }
        Error::UnsupportedSchema { reason, .. } if reason.contains("does not fit `i32`") => {
            return vec![
                "A bound must fit the type the `format` chooses. `int32` holds -2147483648 to 2147483647. Drop the `format` to get `i64`."
                    .to_owned(),
            ];
        }
        Error::UnsupportedSchema { reason, .. } if reason.contains("the union holds") => {
            return vec![
                "A `oneOf` becomes an untagged enum. Serde reads the variants in order and takes the first that fits, so a repeated type is unreachable. Remove the repeated member."
                    .to_owned(),
            ];
        }
        Error::UnsupportedSchema { reason, .. } if reason.contains("`enum`") => {
            return vec![
                "An `enum` names each value once. Remove the repeat.".to_owned(),
                "An integer `enum` value must fit the type the `format` chooses. `int32` holds -2147483648 to 2147483647. Drop the `format` to get `i64`.".to_owned(),
            ];
        }
        Error::UnsupportedSchema { .. } | Error::UnsupportedOperation { .. } => {
            return vec![
                "This spec uses a feature the generator cannot represent yet; simplify the schema/operation or open an issue."
                    .to_owned(),
            ];
        }
        Error::SchemaDepthExceeded { .. } => {
            return vec![
                "A schema nests too deeply; flatten it or split the nested shape into a named component referenced by `$ref`."
                    .to_owned(),
            ];
        }
        Error::InvalidPathParameter { name, .. } => {
            return vec![format!(
                "Add `{{{name}}}` to the path template, or change the parameter's `in:` to `query`, `header`, or `cookie`."
            )];
        }
        Error::UndeclaredPathParameter { name, .. } => {
            return vec![format!(
                "Declare a parameter with `name: {name}`, `in: path`, `required: true`, or remove `{{{name}}}` from the path."
            )];
        }
        Error::UnsupportedSpecVersion { hint, .. }
        | Error::UnsupportedSpecKey { hint, .. }
        | Error::UnsupportedContentType { hint, .. }
        | Error::TypeNameCollision { hint, .. }
        | Error::DuplicateTypeName { hint, .. }
        | Error::PreludeShadowing { hint, .. }
        | Error::OperationTypeCollision { hint, .. }
        | Error::SchemaNameCollision { hint, .. }
        | Error::RecursiveAlias { hint, .. }
        | Error::UnsupportedDefault { hint, .. }
        | Error::OperationNameCollision { hint, .. }
        | Error::InvalidTypeNameSuffix { hint, .. } => {
            return vec![hint.clone()];
        }
        Error::InvalidGeneratedCode { .. } => {
            return vec!["This is an internal bug in oapi-codegen. Please report it with your spec.".to_owned()];
        }
        // `report_error` renders each collected problem on its own, with that
        // problem's own hints, so the aggregate itself adds no hint.
        Error::Validation { .. } => {
            return Vec::new();
        }
    }
}

/// Hints for a failed read, keyed on the underlying IO error kind.
///
/// `what` names the kind of file, and the caller supplies the whole noun phrase
/// (for example `spec file`). Every message below reads it as one noun, so no
/// message adds a word of its own to it.
fn io_read_hints(what: &str, path: &str, kind: ErrorKind) -> Vec<String> {
    match kind {
        ErrorKind::NotFound => {
            return vec![format!(
                "No {what} exists at `{path}`; check the path and your working directory."
            )];
        }
        ErrorKind::PermissionDenied => {
            return vec![format!(
                "Permission denied reading `{path}`; check the file's permissions."
            )];
        }
        _ => {
            return vec![format!("Could not read the {what} at `{path}`.")];
        }
    }
}

/// Hints for a spec that failed to parse, with a special case for documents
/// that are valid YAML/JSON but not OpenAPI 3.
fn parse_spec_hints(message: &str) -> Vec<String> {
    if message.contains("missing field `openapi`") {
        return vec![
            "This file does not look like an OpenAPI 3 document (no top-level `openapi:` field).".to_owned(),
            "oapi-codegen expects an OpenAPI 3.x spec in YAML or JSON, with `openapi`, `info`, and `paths`.".to_owned(),
        ];
    }
    return vec![
        "The spec must be a valid OpenAPI 3 document; the parser message above points at the problem.".to_owned(),
    ];
}

/// Hints for a config that failed to parse.
fn config_hints() -> Vec<String> {
    return vec![
        "The config must be YAML using oapi-codegen's keys, e.g. `output:` and a `generate:` block.".to_owned(),
    ];
}

/// Build hints for empty output based on what the config requested versus what
/// the spec actually contains.
fn empty_output_hints(stats: &SpecStats, generate: &Generate) -> Vec<String> {
    let mut hints = Vec::new();
    if generate.models && stats.schemas == 0 {
        hints.push("`generate.models` is on, but the spec has no `components.schemas` to turn into models.".to_owned());
    }
    let wants_service = generate.std_http_server || generate.client;
    if wants_service && stats.paths == 0 {
        hints.push("A server/client was requested, but the spec declares no paths to turn into operations.".to_owned());
    }
    if generate.server_urls && stats.servers == 0 {
        hints.push(
            "`generate.server-urls` is on, but the spec declares no top-level `servers:` entries to emit.".to_owned(),
        );
    }
    if hints.is_empty() {
        hints.push(
            "Everything requested was filtered out; check your `output-options` include/exclude settings.".to_owned(),
        );
    } else {
        hints
            .push("Add the missing definitions to the spec, or enable a different artifact in your config.".to_owned());
    }
    return hints;
}

/// Format a count with a singular/plural noun, e.g. `1 schema`, `3 schemas`.
fn count(noun: &str, n: usize) -> String {
    if n == 1 {
        return format!("{n} {noun}");
    }
    return format!("{n} {noun}s");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_detects_header_and_comment_only_output() {
        assert!(is_effectively_empty(""));
        assert!(is_effectively_empty(oapi_codegen::emit::HEADER));
        assert!(is_effectively_empty("  // leading whitespace comment\n\n"));
    }

    #[test]
    fn non_empty_output_has_items() {
        assert!(!is_effectively_empty("// header\n\npub struct Foo;\n"));
    }

    /// The real header holds an inner attribute, so a check that only skipped
    /// comment lines would call an item-less file non-empty and write it out.
    #[test]
    fn empty_detects_the_real_header_followed_by_an_item() {
        let code = format!("{}pub struct Foo;\n", oapi_codegen::emit::HEADER);
        assert!(!is_effectively_empty(&code));
    }

    #[test]
    fn count_pluralizes_on_zero_and_many_but_not_one() {
        assert_eq!(count("schema", 0), "0 schemas");
        assert_eq!(count("schema", 1), "1 schema");
        assert_eq!(count("path", 3), "3 paths");
    }

    #[test]
    fn empty_hints_call_out_missing_models() {
        let stats = SpecStats {
            schemas: 0,
            paths: 0,
            servers: 0,
        };
        let generate = Generate {
            models: true,
            ..Default::default()
        };
        let hints = empty_output_hints(&stats, &generate);
        assert!(hints.iter().any(|h| return h.contains("components.schemas")));
    }

    #[test]
    fn empty_hints_call_out_missing_paths_for_service() {
        let stats = SpecStats {
            schemas: 0,
            paths: 0,
            servers: 0,
        };
        let generate = Generate {
            std_http_server: true,
            ..Default::default()
        };
        let hints = empty_output_hints(&stats, &generate);
        assert!(hints.iter().any(|h| return h.contains("no paths")));
    }

    #[test]
    fn empty_hints_call_out_missing_servers_for_server_urls() {
        let stats = SpecStats {
            schemas: 0,
            paths: 0,
            servers: 0,
        };
        let generate = Generate {
            server_urls: true,
            ..Default::default()
        };
        let hints = empty_output_hints(&stats, &generate);
        assert!(hints.iter().any(|h| return h.contains("servers:")));
    }

    #[test]
    fn empty_hints_fall_back_to_filtering_when_nothing_matches() {
        let stats = SpecStats {
            schemas: 5,
            paths: 5,
            servers: 0,
        };
        let generate = Generate {
            models: true,
            ..Default::default()
        };
        let hints = empty_output_hints(&stats, &generate);
        assert!(hints.iter().any(|h| return h.contains("filtered out")));
    }

    #[test]
    fn parse_spec_hint_recognizes_non_openapi_documents() {
        let hints = parse_spec_hints("missing field `openapi` at line 1 column 1");
        assert!(
            hints
                .iter()
                .any(|h| return h.contains("does not look like an OpenAPI 3 document"))
        );
    }

    /// Every caller of [`io_read_hints`] must pass a complete noun phrase. A
    /// caller that passes `referenced` instead of `referenced file` reads as
    /// "No referenced exists at", and one that also adds `file` inside the
    /// message reads as "No referenced file file exists at". This pins the three
    /// call sites so neither mistake returns.
    #[test]
    fn read_hints_name_the_file_kind_one_time() {
        for (what, expected) in [
            ("spec file", "No spec file exists at `x.yaml`"),
            ("config file", "No config file exists at `x.yaml`"),
            ("referenced file", "No referenced file exists at `x.yaml`"),
        ] {
            let hints = io_read_hints(what, "x.yaml", ErrorKind::NotFound);
            assert!(
                hints.iter().any(|hint| return hint.starts_with(expected)),
                "hint for `{what}` should start with `{expected}`, got: {hints:?}",
            );
        }
    }

    #[test]
    fn invalid_path_parameter_hint_guides_the_fix() {
        let err = Error::InvalidPathParameter {
            method: "get".to_owned(),
            path: "/dashboard".to_owned(),
            name: "tz".to_owned(),
        };
        let hints = hints_for(&err);
        assert!(
            hints.iter().any(|h| return h.contains("{tz}") && h.contains("query")),
            "hint should suggest adding the placeholder or changing `in:`, got: {hints:?}",
        );
    }

    /// Each constraint fault must reach its own hint. The arms read the reason
    /// text, so a broad one can take a message meant for a later arm and send the
    /// reader to the wrong fix.
    #[test]
    fn each_constraint_fault_reaches_its_own_hint() {
        let cases = [
            ("the `multipleOf` value `0` is not above zero", "divides by zero"),
            ("the `multipleOf` value `5000000000` does not fit `i32`", "-2147483648"),
            ("the `minimum` value `-5000000000` does not fit `i32`", "-2147483648"),
            (
                "the `pattern` rule does not reach the type this field holds",
                "Drop the `format`",
            ),
            (
                "the `minProperties` rule does not reach the type this field holds",
                "free-form map",
            ),
            (
                "the `uniqueItems` rule does not reach the type this field holds",
                "numbers, strings, or booleans",
            ),
            ("the `enum` gives `1` more than once", "names each value once"),
            (
                "the union holds `Cat` twice, as `Cat` and as `Cat2`",
                "Remove the repeated member",
            ),
            // A schema named `enum` puts that word in the union message too.
            // The union arm must still win.
            (
                "the union holds `enum` twice, as `Enum` and as `Enum2`",
                "Remove the repeated member",
            ),
        ];
        for (reason, wanted) in cases {
            let err = Error::UnsupportedSchema {
                path: "field".to_owned(),
                reason: reason.to_owned(),
            };
            let hints = hints_for(&err);
            assert!(
                hints.iter().any(|hint| return hint.contains(wanted)),
                "`{reason}` should reach a hint holding `{wanted}`, got: {hints:?}",
            );
        }
    }

    #[test]
    fn undeclared_path_parameter_hint_guides_the_fix() {
        let err = Error::UndeclaredPathParameter {
            method: "get".to_owned(),
            path: "/widgets/{id}".to_owned(),
            name: "id".to_owned(),
        };
        let hints = hints_for(&err);
        assert!(
            hints
                .iter()
                .any(|h| return h.contains("in: path") && h.contains("{id}")),
            "hint should suggest declaring the parameter or removing the placeholder, got: {hints:?}",
        );
    }
}
