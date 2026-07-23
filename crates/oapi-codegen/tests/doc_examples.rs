use std::path::Path;
use std::path::PathBuf;

fn markdown_files(dir: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("reading `{dir}` failed: {err}"))
        .filter_map(|entry| return entry.ok())
        .map(|entry| return entry.path())
        .filter(|path| return path.extension().is_some_and(|ext| return ext == "md"))
        .collect();
    assert!(!files.is_empty(), "expected at least one Markdown file under `{dir}`");
    files.sort();
    return files;
}

fn example_readmes(dir: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("reading `{dir}` failed: {err}"))
        .filter_map(|entry| return entry.ok())
        .map(|entry| return entry.path())
        .filter(|path| return path.is_dir())
        .map(|dir| return dir.join("README.md"))
        .filter(|path| return path.exists())
        .collect();
    assert!(
        !files.is_empty(),
        "expected at least one example `README.md` under `{dir}`"
    );
    files.sort();
    return files;
}

#[test]
fn doc_examples_match_cli_behavior() {
    let cases = trycmd::TestCases::new();
    cases.case(Path::new("../../README.md"));
    for path in markdown_files("../../docs") {
        cases.case(path);
    }
    for path in example_readmes("../../examples") {
        cases.case(path);
    }
    cases.insert_var("[VERSION]", env!("CARGO_PKG_VERSION")).unwrap();
}
