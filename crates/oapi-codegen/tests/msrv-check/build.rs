//! Write a module list covering every committed fixture.
//!
//! The list is derived from the directory rather than written by hand, because a
//! hand-written list is a second place to add a fixture, and a file left out
//! of it is not compiled and reports nothing. `tests/generated.rs` in the
//! generator crate keeps such a list, and a coverage test guards it. There is no
//! equivalent guard here, so this crate reads the directory instead.

use std::path::Path;

fn main() {
    // This crate sits at `crates/oapi-codegen/tests/msrv-check`, so the fixture
    // directory is a sibling one level up.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the manifest directory has a parent")
        .join("generated");

    // Re-run when a fixture is added or removed. Without this the module list
    // is cached from the first build and a new file is never compiled.
    println!("cargo:rerun-if-changed={}", root.display());

    let mut stems: Vec<String> = std::fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("cannot read `{}`: {error}", root.display()))
        .map(|entry| {
            return entry.expect("a readable directory entry").path();
        })
        .filter(|path| {
            return path.extension().is_some_and(|extension| return extension == "rs");
        })
        .map(|path| {
            return path
                .file_stem()
                .expect("a `.rs` file has a stem")
                .to_string_lossy()
                .into_owned();
        })
        .collect();
    // Sorted so the module order does not follow directory read order, which no
    // filesystem promises to keep stable. Two runs on one machine then write the
    // same file, and the build cache stays valid.
    //
    // The file is not identical between machines, because each `include!` below
    // holds an absolute path. Nothing needs it to be: the file lives in `OUT_DIR`
    // and is rewritten by this script on each machine.
    stems.sort();

    assert!(!stems.is_empty(), "no fixtures found in `{}`", root.display());

    let mut source = String::new();
    for stem in &stems {
        let path = root.join(format!("{stem}.rs"));
        source.push_str(&format!(
            "pub mod {stem} {{\n    include!(r\"{}\");\n}}\n",
            path.display()
        ));
    }

    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo")).join("fixtures.rs");
    std::fs::write(&out, source).unwrap_or_else(|error| panic!("cannot write `{}`: {error}", out.display()));
}
