use std::path::Path;
use std::path::PathBuf;

const README_MD: &str = "README.md";

fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("reading `{}` failed: {err}", dir.display()))
        .map(|entry| entry.unwrap_or_else(|err| panic!("reading entry under `{}` failed: {err}", dir.display())))
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

fn example_readmes(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("reading `{}` failed: {err}", dir.display()))
        .map(|entry| entry.unwrap_or_else(|err| panic!("reading entry under `{}` failed: {err}", dir.display())))
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
fn doc_examples_match_cli_behavior() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let cases = trycmd::TestCases::new();
    cases.case(repo_root.join(README_MD));
    for path in markdown_files(&repo_root.join("docs")) {
        cases.case(path);
    }
    for path in example_readmes(&repo_root.join("examples")) {
        cases.case(path);
    }
    cases
        .insert_var("[VERSION]", env!("CARGO_PKG_VERSION"))
        .expect("[VERSION] should be a valid trycmd substitution variable");
}
