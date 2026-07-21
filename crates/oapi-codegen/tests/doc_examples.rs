#[test]
fn doc_examples_match_cli_behavior() {
    trycmd::TestCases::new()
        .case("../../README.md")
        .case("../../docs/*.md")
        .case("../../examples/*/README.md")
        .insert_var("[VERSION]", env!("CARGO_PKG_VERSION"))
        .unwrap();
}
