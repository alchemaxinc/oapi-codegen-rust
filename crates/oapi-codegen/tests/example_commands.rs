//! Executes the CLI commands documented in the project's Markdown so they
//! cannot silently rot.
//!
//! Any fenced code block preceded by a `<!-- runnable-example: <dir> -->`
//! comment is treated as runnable: every `oapi-codegen` line in it is executed
//! against the freshly built binary, in a throwaway copy of `<dir>` (relative
//! to the repository root). Adding a new runnable example anywhere in the docs
//! is therefore just a matter of adding that marker — no changes to this test.
//!
//! Illustrative snippets that use placeholder filenames simply omit the marker
//! and are left untouched.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// Marker comment that flags the following fenced block as runnable and names
/// the directory (relative to the repo root) to run its commands in.
const MARKER_PREFIX: &str = "<!-- runnable-example:";

/// A runnable block discovered in the docs: where it came from, the working
/// directory to run it in, and each `oapi-codegen` invocation's arguments.
struct RunnableExample {
    source: PathBuf,
    dir: PathBuf,
    commands: Vec<Vec<String>>,
}

/// Absolute path to the repository root.
fn repo_root() -> PathBuf {
    return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
}

/// Recursively collect every `*.md` file under `dir`, skipping build and
/// dependency directories.
fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|err| {
        panic!("reading `{}` failed: {err}", dir.display());
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|err| {
            panic!("iterating `{}` failed: {err}", dir.display());
        });
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if !matches!(name.to_str(), Some("target" | "node_modules" | ".git")) {
                markdown_files(&path, out);
            }
            continue;
        }
        if path.extension().and_then(std::ffi::OsStr::to_str) == Some("md") {
            out.push(path);
        }
    }
}

/// Extract the directory named by a `runnable-example` marker line, if present.
fn marker_dir(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix(MARKER_PREFIX)?;
    let dir = rest.strip_suffix("-->")?.trim();
    return Some(dir);
}

/// Parse the `oapi-codegen` invocations out of every marked block in `markdown`.
///
/// Walks the file as a small state machine: on a marker line we remember the
/// target directory, wait for the next fenced block to open, then collect every
/// `oapi-codegen` command until the block closes.
fn runnable_examples(source: &Path, root: &Path, markdown: &str) -> Vec<RunnableExample> {
    let mut examples = Vec::new();
    let mut pending_dir: Option<String> = None;
    let mut active: Option<(String, Vec<Vec<String>>)> = None;

    for line in markdown.lines() {
        let trimmed = line.trim();

        if let Some((_, commands)) = active.as_mut() {
            if trimmed.starts_with("```") {
                if let Some((dir, commands)) = active.take() {
                    examples.push(RunnableExample {
                        source: source.to_owned(),
                        dir: root.join(dir),
                        commands,
                    });
                }
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("oapi-codegen ") {
                commands.push(rest.split_whitespace().map(str::to_owned).collect());
            }
            continue;
        }

        if let Some(dir) = pending_dir.take() {
            if trimmed.starts_with("```") {
                active = Some((dir, Vec::new()));
            } else {
                pending_dir = Some(dir);
            }
            continue;
        }

        if let Some(dir) = marker_dir(line) {
            pending_dir = Some(dir.to_owned());
        }
    }

    return examples;
}

/// Recursively copy `src` into `dst`, creating directories as needed.
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap_or_else(|err| {
        panic!("creating `{}` failed: {err}", dst.display());
    });
    let entries = std::fs::read_dir(src).unwrap_or_else(|err| {
        panic!("reading `{}` failed: {err}", src.display());
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|err| {
            panic!("iterating `{}` failed: {err}", src.display());
        });
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap_or_else(|err| {
                panic!("copying `{}` failed: {err}", from.display());
            });
        }
    }
}

#[test]
fn documented_commands_run() {
    let root = repo_root();
    let mut files = Vec::new();
    markdown_files(&root, &mut files);
    files.sort();

    let mut examples = Vec::new();
    for file in &files {
        let markdown = std::fs::read_to_string(file).unwrap_or_else(|err| {
            panic!("reading `{}` failed: {err}", file.display());
        });
        examples.extend(runnable_examples(file, &root, &markdown));
    }
    assert!(
        !examples.is_empty(),
        "no `<!-- runnable-example: ... -->` blocks found in any Markdown file",
    );

    for (offset, example) in examples.iter().enumerate() {
        assert!(
            example.dir.is_dir(),
            "`{}` marks a runnable example in `{}`, which does not exist",
            example.source.display(),
            example.dir.display(),
        );
        assert!(
            !example.commands.is_empty(),
            "`{}` marks a runnable example with no `oapi-codegen` commands",
            example.source.display(),
        );

        let workspace = std::env::temp_dir().join(format!("oapi-codegen-docs-{}-{offset}", std::process::id()));
        copy_dir(&example.dir, &workspace);

        for args in &example.commands {
            let status = Command::new(env!("CARGO_BIN_EXE_oapi-codegen"))
                .args(args)
                .current_dir(&workspace)
                .status()
                .unwrap_or_else(|err| {
                    panic!(
                        "spawning documented command `oapi-codegen {}` (from {}) failed: {err}",
                        args.join(" "),
                        example.source.display(),
                    );
                });
            assert!(
                status.success(),
                "documented command failed: `oapi-codegen {}` (from {})",
                args.join(" "),
                example.source.display(),
            );
        }

        std::fs::remove_dir_all(&workspace).unwrap_or_else(|err| {
            panic!("cleaning up `{}` failed: {err}", workspace.display());
        });
    }
}
