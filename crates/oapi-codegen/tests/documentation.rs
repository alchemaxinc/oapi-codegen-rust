use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

const README_MD: &str = "README.md";

/// Serializes tests that mutate the process working directory. `set_current_dir`
/// is process-global, so the doc tests that switch directories must not run
/// concurrently or they will observe each other's directory.
static WORKING_DIR_LOCK: Mutex<()> = Mutex::new(());

struct WorkingDirGuard {
    previous: PathBuf,
    cleanup: Option<PathBuf>,
}

impl WorkingDirGuard {
    fn change_to(target: &Path) -> Self {
        let previous = std::env::current_dir().unwrap_or_else(|err| panic!("reading current directory failed: {err}"));
        std::env::set_current_dir(target)
            .unwrap_or_else(|err| panic!("switching current directory to `{}` failed: {err}", target.display()));
        return Self {
            previous,
            cleanup: None,
        };
    }

    /// Removes `path` on drop, but only if it does not already exist now. This
    /// keeps a developer's pre-existing directory untouched while cleaning up
    /// anything the case generates.
    fn remove_on_drop(mut self, path: PathBuf) -> Self {
        if !path.exists() {
            self.cleanup = Some(path);
        }
        return self;
    }
}

impl Drop for WorkingDirGuard {
    fn drop(&mut self) {
        if let Some(path) = &self.cleanup {
            let _ = std::fs::remove_dir_all(path);
        }
        let _ = std::env::set_current_dir(&self.previous);
    }
}

/// Acquires the process-wide working-directory lock, recovering from poisoning
/// so a panic in one case does not cascade into unrelated test failures.
fn lock_working_dir() -> std::sync::MutexGuard<'static, ()> {
    return WORKING_DIR_LOCK
        .lock()
        .unwrap_or_else(|poisoned| return poisoned.into_inner());
}

fn collect_markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("reading `{}` failed: {err}", dir.display()))
        .map(|entry| return entry.unwrap_or_else(|err| panic!("reading entry under `{}` failed: {err}", dir.display())))
        .map(|entry| return entry.path())
        .filter(|path| return path.extension().is_some_and(|ext| return ext == "md"))
        .collect();

    assert!(
        !files.is_empty(),
        "expected at least one Markdown file under `{}`",
        dir.display()
    );
    files.sort();
    return files;
}

fn collect_example_readmes(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("reading `{}` failed: {err}", dir.display()))
        .map(|entry| return entry.unwrap_or_else(|err| panic!("reading entry under `{}` failed: {err}", dir.display())))
        .map(|entry| return entry.path())
        .filter(|path| return path.is_dir())
        .map(|dir| return dir.join(README_MD))
        .filter(|path| return path.exists())
        .collect();

    assert!(
        !files.is_empty(),
        "expected at least one example `README.md` under `{}`",
        dir.display()
    );
    files.sort();
    return files;
}

#[test]
fn documentation_root_readme_examples() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo_root = std::fs::canonicalize(&repo_root)
        .unwrap_or_else(|err| panic!("resolving repo root `{}` failed: {err}", repo_root.display()));

    // trycmd's default README workflow uses a sibling `README.in/` directory; we avoid adding that at the repo root.
    // Instead, run `README.md` from the repo root and clean up any `generated/` output it creates.
    let _lock = lock_working_dir();
    let _cwd = WorkingDirGuard::change_to(&repo_root).remove_on_drop(repo_root.join("generated"));
    trycmd::TestCases::new().case(repo_root.join(README_MD));
}

#[test]
fn documentation_match_cli_behavior() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let cases = trycmd::TestCases::new();
    for path in collect_markdown_files(&repo_root.join("docs")) {
        cases.case(path);
    }
    cases
        .insert_var("[VERSION]", env!("CARGO_PKG_VERSION"))
        .unwrap_or_else(|err| panic!("[VERSION] should be a valid trycmd substitution variable: {err}"));
    drop(cases);

    // Example READMEs document commands meant to be run from the example's own
    // directory (relative config and spec paths), so each one runs with the
    // working directory pointed at that folder. Regenerating overwrites the
    // committed `generated/` output in place, which is deterministic.
    let _lock = lock_working_dir();
    for readme in collect_example_readmes(&repo_root.join("examples")) {
        let dir = readme
            .parent()
            .unwrap_or_else(|| panic!("example README `{}` has no parent directory", readme.display()));
        let _cwd = WorkingDirGuard::change_to(dir).remove_on_drop(dir.join("generated"));
        trycmd::TestCases::new().case(&readme);
    }
}
