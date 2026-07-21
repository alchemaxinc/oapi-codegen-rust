//! Executes the CLI commands documented in the bookstore example's README so
//! they cannot silently rot.
//!
//! The README is the source of truth: this test extracts every `oapi-codegen`
//! command from its fenced code block, runs each against the freshly built
//! binary in a throwaway copy of the example directory, and asserts success. If
//! a documented flag, argument order, or filename drifts from the real CLI, the
//! command fails here.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// Absolute path to the bookstore example directory.
fn bookstore_dir() -> PathBuf {
    return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/bookstore");
}

/// Extract the `oapi-codegen` invocations (as argument vectors) from the
/// bookstore README's fenced code block.
fn documented_commands(readme: &str) -> Vec<Vec<String>> {
    return readme
        .lines()
        .map(str::trim)
        .filter(|line| return line.starts_with("oapi-codegen "))
        .map(|line| {
            return line.split_whitespace().skip(1).map(str::to_owned).collect();
        })
        .collect();
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
fn bookstore_readme_commands_run() {
    let dir = bookstore_dir();
    let readme = std::fs::read_to_string(dir.join("README.md")).unwrap_or_else(|err| {
        panic!("reading bookstore README failed: {err}");
    });
    let commands = documented_commands(&readme);
    assert!(
        !commands.is_empty(),
        "no `oapi-codegen` commands found in the bookstore README"
    );

    let workspace = std::env::temp_dir().join(format!("oapi-codegen-readme-{}", std::process::id()));
    copy_dir(&dir, &workspace);

    for args in &commands {
        let status = Command::new(env!("CARGO_BIN_EXE_oapi-codegen"))
            .args(args)
            .current_dir(&workspace)
            .status()
            .unwrap_or_else(|err| {
                panic!(
                    "spawning documented command `oapi-codegen {}` failed: {err}",
                    args.join(" ")
                );
            });
        assert!(
            status.success(),
            "documented command failed: `oapi-codegen {}`",
            args.join(" ")
        );
    }

    std::fs::remove_dir_all(&workspace).unwrap_or_else(|err| {
        panic!("cleaning up `{}` failed: {err}", workspace.display());
    });
}
