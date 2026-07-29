//! Terminal output for the CLI with guided, colored error reports.
//!
//! This module writes to stderr through [`anstream`].
//! It strips ANSI styling when stderr is not a terminal or `NO_COLOR` is set.
//! It uses [`owo_colors`] for styling.
//! Each failure states what went wrong and gives a next step when possible.

use std::io::ErrorKind;
use std::path::Path;

use anstream::eprint;
use anstream::eprintln;
use oapi_codegen::Error;
use oapi_codegen::config::Generate;
use oapi_codegen::deps::Dependency;
use owo_colors::OwoColorize;

/// Example `generate:` block shown when a configuration enables no artifacts. Each
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

/// Return `true` when generated `code` contains no items.
/// The code can contain only the header comment and blank lines.
/// The CLI reports this result as a failure.
pub fn is_effectively_empty(code: &str) -> bool {
    return code.lines().all(|line| {
        let trimmed = line.trim();
        return trimmed.is_empty() || trimmed.starts_with("//");
    });
}

/// Print a guided, styled error report for a generator `err` to stderr.
pub fn report_error(err: &Error) {
    eprintln!("{} {}", "error:".red().bold(), err.bold());
    for hint in hints_for(err) {
        eprintln!("  {} {hint}", "hint:".cyan().bold());
    }
}

/// Report that no output destination was given and show how to fix it.
pub fn report_no_output() {
    eprintln!("{} no output destination was given.", "error:".red().bold());
    eprintln!(
        "  {} pass `--output-file <file>` on the command line, or set `output:` in the configuration.",
        "hint:".cyan().bold()
    );
}

/// Report that a loaded configuration enables no artifacts and show how to fix it.
pub fn report_no_artifacts(config: &Path) {
    eprintln!(
        "{} the configuration `{}` enables no artifact.",
        "error:".red().bold(),
        config.display()
    );
    eprintln!(
        "  {} enable at least one artifact under `generate:`",
        "hint:".cyan().bold()
    );
    print_snippet(GENERATE_EXAMPLE);
}

/// Print a multi-line code snippet with indentation under a hint.
fn print_snippet(snippet: &str) {
    for line in snippet.lines() {
        eprintln!("      {}", line.dimmed());
    }
}

/// Report that generation produced no code.
/// The report compares the requested artifacts with the specification contents.
pub fn report_empty_output(spec: &Path, stats: &SpecStats, generate: &Generate) {
    eprintln!(
        "{} generation of `{}` produced no code.",
        "error:".red().bold(),
        spec.display()
    );
    eprintln!(
        "  the specification declares {} and {}.",
        count("schema", stats.schemas).bold(),
        count("path", stats.paths).bold()
    );
    for hint in empty_output_hints(stats, generate) {
        eprintln!("  {} {hint}", "hint:".cyan().bold());
    }
}

/// Report that the CLI wrote output to `path`.
pub fn report_wrote(path: &Path) {
    eprintln!("{} wrote {}", "✓".green().bold(), path.display());
}

/// After a successful write, list the external crates the generated code
/// references. Print nothing when the output references no external crates.
pub fn report_dependencies(deps: &[Dependency]) {
    if deps.is_empty() {
        return;
    }
    eprintln!(
        "  {} add the crates that the generated code references to Cargo.toml:",
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

/// Ask whether to run the `cargo add` commands now.
/// Return `false` on EOF or a non-affirmative answer.
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

/// Report that the CLI adds a dependency with `cargo add`.
pub fn report_installing(dep: &Dependency) {
    eprintln!("  {} {}", "+".green().bold(), dep.cargo_add().dimmed());
}

/// Report that a `cargo add` command failed.
/// The output file is already written, so dependency installation remains a warning.
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
            return io_read_hints("spec", path, source.kind());
        }
        Error::ReadConfig { path, source } => {
            return io_read_hints("config", path, source.kind());
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
            return vec![format!("Make sure that the directory for `{path}` is writable.")];
        }
        Error::Unimplemented(_) => {
            return vec!["This generation mode is not supported yet.".to_owned()];
        }
        Error::UnresolvedRef(_) | Error::UnsupportedRef { .. } => {
            return vec![
                "Supported references are local `#/components/...` pointers and same-directory cross-file refs."
                    .to_owned(),
            ];
        }
        Error::UnsupportedSchema { .. } | Error::UnsupportedOperation { .. } => {
            return vec![
                "This specification uses an unsupported feature. Simplify the schema or operation, or open an issue."
                    .to_owned(),
            ];
        }
        Error::SchemaDepthExceeded { .. } => {
            return vec![
                "A schema nests too deeply. Flatten it or split the nested shape into a named component referenced by `$ref`."
                    .to_owned(),
            ];
        }
        Error::InvalidPathParameter { name, .. } => {
            return vec![format!(
                "Add `{{{name}}}` to the path template. Or change the parameter's `in:` to `query`, `header`, or `cookie`."
            )];
        }
        Error::UndeclaredPathParameter { name, .. } => {
            return vec![format!(
                "Declare a parameter with `name: {name}`, `in: path`, and `required: true`. Or remove `{{{name}}}` from the path."
            )];
        }
        Error::TypeNameCollision { hint, .. } => {
            return vec![hint.clone()];
        }
        Error::InvalidGeneratedCode { .. } => {
            return vec!["This is an internal bug in oapi-codegen. Report it with your specification.".to_owned()];
        }
    }
}

/// Hints for a failed read, keyed on the underlying IO error kind.
fn io_read_hints(what: &str, path: &str, kind: ErrorKind) -> Vec<String> {
    match kind {
        ErrorKind::NotFound => {
            return vec![format!(
                "No {what} file exists at `{path}`. Make sure that the path and working directory are correct."
            )];
        }
        ErrorKind::PermissionDenied => {
            return vec![format!(
                "Permission denied while reading `{path}`. Make sure that the file permissions allow reading."
            )];
        }
        _ => {
            return vec![format!("The CLI could not read the {what} at `{path}`.")];
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
        "The specification must be a valid OpenAPI 3 document. The parser message above points at the problem."
            .to_owned(),
    ];
}

/// Hints for a configuration that failed to parse.
fn config_hints() -> Vec<String> {
    return vec![
        "The configuration must use YAML and oapi-codegen keys, such as `output:` and a `generate:` block.".to_owned(),
    ];
}

/// Build hints for empty output based on what the configuration requested versus what
/// the spec actually contains.
fn empty_output_hints(stats: &SpecStats, generate: &Generate) -> Vec<String> {
    let mut hints = Vec::new();
    if generate.models && stats.schemas == 0 {
        hints.push("`generate.models` is on, but the specification has no `components.schemas` for models.".to_owned());
    }
    let wants_service = generate.std_http_server || generate.client;
    if wants_service && stats.paths == 0 {
        hints.push("A server or client was requested, but the specification declares no paths.".to_owned());
    }
    if generate.server_urls && stats.servers == 0 {
        hints.push(
            "`generate.server-urls` is on, but the specification declares no top-level `servers:` entries.".to_owned(),
        );
    }
    if hints.is_empty() {
        hints.push(
            "The filters removed every requested item. Make sure that `output-options` includes the required items."
                .to_owned(),
        );
    } else {
        hints.push(
            "Add the missing definitions to the specification, or enable another artifact in your configuration."
                .to_owned(),
        );
    }
    return hints;
}

/// Format a count with a singular/plural noun, for example `1 schema`, `3 schemas`.
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
        assert!(is_effectively_empty(
            "// Code generated by oapi-codegen-rust. DO NOT EDIT.\n\n"
        ));
        assert!(is_effectively_empty("  // leading whitespace comment\n\n"));
    }

    #[test]
    fn non_empty_output_has_items() {
        assert!(!is_effectively_empty("// header\n\npub struct Foo;\n"));
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
        assert!(hints.iter().any(|h| return h.contains("filters removed")));
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
            "the hint must suggest adding the placeholder or changing `in:`, got: {hints:?}",
        );
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
            "the hint must suggest declaring the parameter or removing the placeholder, got: {hints:?}",
        );
    }
}
