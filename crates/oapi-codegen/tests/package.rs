//! The shape of the split output, and the file operations over it.
//!
//! The golden files under `tests/generated/package_*` pin what the split output
//! contains and prove it compiles. These cases cover what a golden cannot: that
//! the split loses nothing the flat layout emits, and that writing and checking
//! a package reconcile the companion directory rather than only its files.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use oapi_codegen::PackageDrift;

/// A spec with one operation, one component schema, and a security scheme, so
/// its output fills every module the emitter can produce.
const SPEC: &str = "\
openapi: 3.0.3
info:
  title: Demo
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /widgets/{id}:
    get:
      operationId: getWidget
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: The widget
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/Widget'
components:
  schemas:
    Widget:
      type: object
      required: [name]
      properties:
        name:
          type: string
";

/// A models-only spec, which produces no operations and so no companion files.
const MODELS_ONLY_SPEC: &str = "\
openapi: 3.0.3
info:
  title: Demo
  version: 1.0.0
paths: {}
components:
  schemas:
    Widget:
      type: object
      properties:
        name:
          type: string
";

/// A directory under the temp directory that goes away with the test.
struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(test_name: &str) -> Self {
        let elapsed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|err| panic!("system clock should be after the Unix epoch: {err}"));
        let unique = format!(
            "oapi-codegen-package-{test_name}-{}-{}",
            std::process::id(),
            elapsed.as_nanos(),
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).unwrap_or_else(|err| panic!("creating `{}` failed: {err}", path.display()));
        return Self { path };
    }

    /// Write `contents` to `file`, creating the directories it needs.
    fn write(&self, file: &str, contents: &str) -> PathBuf {
        let path = self.path.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|err| panic!("creating `{}` failed: {err}", parent.display()));
        }
        std::fs::write(&path, contents).unwrap_or_else(|err| panic!("writing `{}` failed: {err}", path.display()));
        return path;
    }

    /// Write `SPEC` and give back the path to it.
    fn spec(&self) -> PathBuf {
        return self.write("spec.yaml", SPEC);
    }

    fn join(&self, file: &str) -> PathBuf {
        return self.path.join(file);
    }

    /// The output path every case here generates to.
    fn output(&self) -> PathBuf {
        return self.join("restapi.rs");
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A configuration that turns on every generator, so the output spans models,
/// operations, server, and client.
fn full_config() -> oapi_codegen::Config {
    return oapi_codegen::Config {
        generate: oapi_codegen::config::Generate {
            models: true,
            server_urls: true,
            std_http_server: true,
            client: true,
            ..Default::default()
        },
        ..Default::default()
    };
}

/// A configuration with no operations to split, so the output is one file.
fn models_only_config() -> oapi_codegen::Config {
    return oapi_codegen::Config {
        generate: oapi_codegen::config::Generate {
            models: true,
            ..Default::default()
        },
        ..Default::default()
    };
}

/// Generate `spec` as a package, failing the test with the generator's message.
fn package_for(spec: &Path, config: &oapi_codegen::Config, output: &Path) -> oapi_codegen::GeneratedPackage {
    return oapi_codegen::generate_package(spec, config, output)
        .unwrap_or_else(|err| panic!("generating `{}` failed: {err}", spec.display()));
}

/// The relative path of every companion file, sorted.
fn child_paths(package: &oapi_codegen::GeneratedPackage) -> Vec<String> {
    let mut paths: Vec<String> = package
        .children()
        .iter()
        .map(|child| return child.path().to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();
    return paths;
}

/// The name of every item `source` declares.
///
/// This reads the code as text on purpose. The point is to compare two
/// renderings of the same generator run, and a scan that treats both the same
/// way answers that without a parser.
fn declared_names(source: &str) -> BTreeSet<String> {
    const VISIBILITIES: &[&str] = &["pub(crate) ", "pub(super) ", "pub "];
    const KEYWORDS: &[&str] = &["struct ", "enum ", "trait ", "fn ", "type ", "const ", "static "];

    let mut names = BTreeSet::new();
    for line in source.lines() {
        let mut rest = line.trim();
        for visibility in VISIBILITIES {
            if let Some(tail) = rest.strip_prefix(visibility) {
                rest = tail;
                break;
            }
        }
        rest = rest.strip_prefix("async ").unwrap_or(rest);
        for keyword in KEYWORDS {
            let Some(tail) = rest.strip_prefix(keyword) else {
                continue;
            };
            let name: String = tail
                .chars()
                .take_while(|character| return character.is_alphanumeric() || *character == '_')
                .collect();
            if !name.is_empty() {
                names.insert(name);
            }
            break;
        }
    }
    return names;
}

#[test]
fn package_holds_one_module_per_concern_and_one_file_per_operation() {
    let dir = TestDir::new("layout");
    let package = package_for(&dir.spec(), &full_config(), &dir.output());

    assert_eq!(
        child_paths(&package),
        vec![
            "restapi/client.rs",
            "restapi/client/get_widget.rs",
            "restapi/models.rs",
            "restapi/operations.rs",
            "restapi/operations/get_widget.rs",
            "restapi/server.rs",
            "restapi/server/get_widget.rs",
            "restapi/server_urls.rs",
        ],
    );
    assert_eq!(package.file_count(), 9, "the root file counts too");
}

#[test]
fn the_root_file_mounts_and_reexports_every_module() {
    let dir = TestDir::new("root");
    let package = package_for(&dir.spec(), &full_config(), &dir.output());
    let root = package.root_source();

    for module in ["models", "server_urls", "operations", "server", "client"] {
        assert!(
            root.contains(&format!("#[path = \"restapi/{module}.rs\"]\nmod {module};")),
            "the root file should mount `{module}`, but holds:\n{root}",
        );
        assert!(
            root.contains(&format!("pub use {module}::*;")),
            "the root file should re-export `{module}`, but holds:\n{root}",
        );
    }
}

#[test]
fn the_root_file_names_only_modules_that_have_content() {
    let dir = TestDir::new("root_sparse");
    let config = oapi_codegen::Config {
        generate: oapi_codegen::config::Generate {
            std_http_server: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let package = package_for(&dir.spec(), &config, &dir.output());

    assert!(
        !package.root_source().contains("mod client;"),
        "a run without the client generator should not mount a client module",
    );
    assert!(
        !child_paths(&package).iter().any(|path| return path.contains("client")),
        "a run without the client generator should produce no client files",
    );
}

#[test]
fn splitting_the_output_declares_the_same_items_as_the_flat_layout() {
    let dir = TestDir::new("items");
    let spec = dir.spec();
    let config = full_config();

    let flat = oapi_codegen::generate(&spec, &config).unwrap_or_else(|err| panic!("generating flat failed: {err}"));
    let package = package_for(&spec, &config, &dir.output());

    assert_eq!(
        declared_names(&package.combined_source()),
        declared_names(&flat),
        "the split output should declare exactly what the flat output declares",
    );
}

#[test]
fn a_run_without_operations_produces_a_single_file() {
    let dir = TestDir::new("models_only");
    let spec = dir.write("spec.yaml", MODELS_ONLY_SPEC);
    let config = models_only_config();

    let package = package_for(&spec, &config, &dir.output());
    let flat = oapi_codegen::generate(&spec, &config).unwrap_or_else(|err| panic!("generating flat failed: {err}"));

    assert!(
        package.children().is_empty(),
        "a models-only run needs no companion files"
    );
    assert_eq!(
        package.root_source(),
        flat,
        "a models-only run should write what it has always written",
    );
}

#[test]
fn a_run_without_operations_removes_the_companion_directory_an_earlier_run_left() {
    let dir = TestDir::new("models_only_cleanup");
    let output = dir.output();
    let split = package_for(&dir.spec(), &full_config(), &output);
    oapi_codegen::write_package(&output, &split).unwrap_or_else(|err| panic!("writing the split failed: {err}"));

    let spec = dir.write("models.yaml", MODELS_ONLY_SPEC);
    let package = package_for(&spec, &models_only_config(), &output);
    oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("writing the models failed: {err}"));

    assert!(
        !dir.join("restapi").exists(),
        "the companion directory an earlier run left should go",
    );
    assert!(output.is_file(), "the root file should hold the models-only output");
}

#[test]
fn a_leftover_generated_file_is_drift_for_a_run_without_operations() {
    let dir = TestDir::new("models_only_stale_check");
    let output = dir.output();
    let spec = dir.write("models.yaml", MODELS_ONLY_SPEC);
    let package = package_for(&spec, &models_only_config(), &output);
    oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("writing failed: {err}"));

    let stale = dir.write(
        "restapi/operations.rs",
        "// Code generated by oapi-codegen-rust. DO NOT EDIT.\n",
    );

    assert_eq!(
        oapi_codegen::check_package(&output, &package).unwrap_or_else(|err| panic!("checking failed: {err}")),
        PackageDrift::Stale(stale),
        "a check should report a generated file beside an output the run no longer splits",
    );
}

#[test]
fn a_run_without_operations_accepts_an_output_path_with_no_extension() {
    let dir = TestDir::new("models_only_no_extension");
    let spec = dir.write("models.yaml", MODELS_ONLY_SPEC);
    // Such a path can carry no companion directory, and a run that produces no
    // companion files does not need one.
    let output = dir.join("restapi");
    let package = package_for(&spec, &models_only_config(), &output);

    oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("writing failed: {err}"));

    assert!(output.is_file(), "the root file should exist");
    assert_eq!(
        oapi_codegen::check_package(&output, &package).unwrap_or_else(|err| panic!("checking failed: {err}")),
        PackageDrift::None,
        "a check right after a write should find no drift",
    );
}

#[test]
fn writing_a_package_creates_every_file() {
    let dir = TestDir::new("write");
    let output = dir.output();
    let package = package_for(&dir.spec(), &full_config(), &output);

    oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("writing failed: {err}"));

    assert!(output.is_file(), "the root file should exist");
    for child in package.children() {
        let path = dir.path.join(child.path());
        assert!(path.is_file(), "`{}` should exist", path.display());
    }
    assert_eq!(
        oapi_codegen::check_package(&output, &package).unwrap_or_else(|err| panic!("checking failed: {err}")),
        PackageDrift::None,
        "a check right after a write should find no drift",
    );
}

#[test]
fn writing_a_package_removes_a_generated_file_the_run_no_longer_produces() {
    let dir = TestDir::new("cleanup");
    let output = dir.output();
    let package = package_for(&dir.spec(), &full_config(), &output);
    oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("writing failed: {err}"));

    let stale = dir.write(
        "restapi/operations/removed_widget.rs",
        "// Code generated by oapi-codegen-rust. DO NOT EDIT.\n",
    );
    let nested = dir.write(
        "restapi/gone/deeper.rs",
        "// Code generated by oapi-codegen-rust. DO NOT EDIT.\n",
    );

    oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("rewriting failed: {err}"));

    assert!(!stale.exists(), "a generated file the run does not produce should go");
    assert!(!nested.exists(), "the walk should reach nested directories");
    assert!(
        !dir.join("restapi/gone").exists(),
        "a directory the cleanup empties should go too",
    );
    assert!(output.is_file(), "the root file should survive");
}

#[test]
fn a_leftover_generated_file_is_drift() {
    let dir = TestDir::new("stale_check");
    let output = dir.output();
    let package = package_for(&dir.spec(), &full_config(), &output);
    oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("writing failed: {err}"));

    let stale = dir.write(
        "restapi/operations/removed_widget.rs",
        "// Code generated by oapi-codegen-rust. DO NOT EDIT.\n",
    );

    assert_eq!(
        oapi_codegen::check_package(&output, &package).unwrap_or_else(|err| panic!("checking failed: {err}")),
        PackageDrift::Stale(stale),
    );
}

#[test]
fn a_missing_or_changed_companion_file_is_drift() {
    let dir = TestDir::new("child_drift");
    let output = dir.output();
    let package = package_for(&dir.spec(), &full_config(), &output);

    assert_eq!(
        oapi_codegen::check_package(&output, &package).unwrap_or_else(|err| panic!("checking failed: {err}")),
        PackageDrift::Absent(output.clone()),
        "nothing on disk should report the root as absent",
    );

    oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("writing failed: {err}"));

    let child = dir.join("restapi/models.rs");
    std::fs::remove_file(&child).unwrap_or_else(|err| panic!("removing `{}` failed: {err}", child.display()));
    assert_eq!(
        oapi_codegen::check_package(&output, &package).unwrap_or_else(|err| panic!("checking failed: {err}")),
        PackageDrift::Absent(child.clone()),
        "a companion file the run produces should be reported when it is missing",
    );

    dir.write(
        "restapi/models.rs",
        "// Code generated by oapi-codegen-rust. DO NOT EDIT.\n// edited\n",
    );
    assert_eq!(
        oapi_codegen::check_package(&output, &package).unwrap_or_else(|err| panic!("checking failed: {err}")),
        PackageDrift::Differs(child),
        "an edited companion file should be reported",
    );
}

#[test]
fn a_file_the_generator_did_not_write_is_refused() {
    let dir = TestDir::new("unowned");
    let output = dir.output();
    let package = package_for(&dir.spec(), &full_config(), &output);
    oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("writing failed: {err}"));

    let handwritten = dir.write("restapi/notes.rs", "// mine, not the generator's\n");

    let error = oapi_codegen::write_package(&output, &package)
        .expect_err("writing should refuse to touch a directory holding a file it does not own");
    assert!(
        matches!(&error, oapi_codegen::Error::UnownedOutput { path } if *path == handwritten.display().to_string()),
        "expected `UnownedOutput` for `{}`, got: {error}",
        handwritten.display(),
    );
    assert!(handwritten.is_file(), "the refused file must survive");

    let error =
        oapi_codegen::check_package(&output, &package).expect_err("checking should refuse the same file as writing");
    assert!(
        matches!(&error, oapi_codegen::Error::UnownedOutput { path } if *path == handwritten.display().to_string()),
        "expected `UnownedOutput` for `{}`, got: {error}",
        handwritten.display(),
    );
}

#[test]
fn a_hand_written_file_where_a_generated_one_belongs_is_refused_and_kept() {
    let dir = TestDir::new("unowned_expected");
    let output = dir.output();
    let package = package_for(&dir.spec(), &full_config(), &output);

    // The path a generated module occupies, holding somebody's work. A write
    // that replaced it before checking ownership would lose that work.
    let mine = "// mine, not the generator's\n";
    let occupied = dir.write("restapi/models.rs", mine);

    let error =
        oapi_codegen::write_package(&output, &package).expect_err("writing must not replace a file it does not own");
    assert!(
        matches!(&error, oapi_codegen::Error::UnownedOutput { path } if *path == occupied.display().to_string()),
        "expected `UnownedOutput` for `{}`, got: {error}",
        occupied.display(),
    );
    assert_eq!(
        std::fs::read_to_string(&occupied).unwrap_or_else(|err| panic!("reading failed: {err}")),
        mine,
        "the refused file must be untouched",
    );
    assert!(
        !output.exists(),
        "a refused run must write nothing, not even the root file",
    );

    let error =
        oapi_codegen::check_package(&output, &package).expect_err("checking must refuse the same file as writing");
    assert!(
        matches!(&error, oapi_codegen::Error::UnownedOutput { .. }),
        "expected `UnownedOutput`, got: {error}",
    );
}

#[test]
fn a_symlink_in_the_companion_directory_is_refused() {
    #[cfg(unix)]
    {
        let dir = TestDir::new("symlink");
        let output = dir.output();
        let package = package_for(&dir.spec(), &full_config(), &output);
        oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("writing failed: {err}"));

        // A link the walk must not follow. Its target carries the generated
        // marker, so an ownership test that read through the link would delete
        // a file outside the directory this run owns.
        let outside = dir.write("outside.rs", "// Code generated by oapi-codegen-rust. DO NOT EDIT.\n");
        let link = dir.join("restapi/linked.rs");
        std::os::unix::fs::symlink(&outside, &link)
            .unwrap_or_else(|err| panic!("linking `{}` failed: {err}", link.display()));

        let error = oapi_codegen::write_package(&output, &package).expect_err("writing must refuse a link");
        assert!(
            matches!(&error, oapi_codegen::Error::UnownedOutput { path } if *path == link.display().to_string()),
            "expected `UnownedOutput` for `{}`, got: {error}",
            link.display(),
        );
        assert!(outside.is_file(), "the link's target must survive");
        assert!(link.exists(), "the link itself must survive");
    }
}

#[test]
fn an_output_path_with_no_extension_is_rejected_before_anything_is_written() {
    let dir = TestDir::new("no_extension");
    let spec = dir.spec();
    // The companion directory is named after the output's stem, so an output
    // with no extension would put the directory and the root file at one path.
    let output = dir.join("restapi");

    let error = oapi_codegen::generate_package(&spec, &full_config(), &output)
        .expect_err("an output path with no extension cannot carry a companion directory");
    assert!(
        matches!(&error, oapi_codegen::Error::UnsplittableOutput { path } if *path == output.display().to_string()),
        "expected `UnsplittableOutput` for `{}`, got: {error}",
        output.display(),
    );
    assert!(!output.exists(), "nothing may be written");
}

#[test]
fn an_operation_named_after_a_keyword_gets_a_raw_module_and_a_plain_file() {
    let dir = TestDir::new("keywords");
    let spec = dir.write(
        "spec.yaml",
        "\
openapi: 3.0.3
info:
  title: Demo
  version: 1.0.0
paths:
  /type:
    get:
      operationId: type
      responses:
        '200':
          description: ok
  /mod:
    get:
      operationId: mod
      responses:
        '200':
          description: ok
",
    );
    let package = package_for(&spec, &full_config(), &dir.output());

    let paths = child_paths(&package);
    assert!(
        paths.contains(&"restapi/operations/type.rs".to_owned()),
        "a keyword operation keeps its own file name: {paths:?}",
    );
    // `mod.rs` beside `operations.rs` is the E0761 ambiguity, so only that name
    // moves aside. The module item still carries the operation's identifier.
    assert!(
        paths.contains(&"restapi/operations/mod_.rs".to_owned()),
        "`mod` must not claim `mod.rs`: {paths:?}",
    );

    let operations = package
        .children()
        .iter()
        .find(|child| return child.path().ends_with("operations.rs"))
        .unwrap_or_else(|| panic!("the package should hold an operations module: {paths:?}"));
    assert!(
        operations.source().contains("mod r#type;"),
        "a keyword module needs its raw form:\n{}",
        operations.source(),
    );
    assert!(
        operations
            .source()
            .contains("#[path = \"operations/mod_.rs\"]\nmod r#mod;"),
        "the explicit path ties the moved file to the raw module:\n{}",
        operations.source(),
    );
}

#[test]
fn a_child_path_that_escapes_the_companion_directory_is_refused() {
    let dir = TestDir::new("escape");
    let output = dir.output();
    let victim = dir.write("victim.rs", "// mine, not the generator's\n");

    // The emitter never builds such a path, but the package types are public
    // and every write below trusts that a child lands under the companion
    // directory.
    for escape in ["../victim.rs", "restapi/../../victim.rs"] {
        let package = oapi_codegen::GeneratedPackage::new(
            "// Code generated by oapi-codegen-rust. DO NOT EDIT.\n",
            vec![oapi_codegen::GeneratedFile::new(
                escape,
                "// Code generated by oapi-codegen-rust. DO NOT EDIT.\n",
            )],
        );

        let error = match oapi_codegen::write_package(&output, &package) {
            Err(error) => error,
            Ok(()) => panic!("writing `{escape}` should have been refused"),
        };
        assert!(
            matches!(&error, oapi_codegen::Error::OutsideOutput { .. }),
            "expected `OutsideOutput` for `{escape}`, got: {error}",
        );
        assert!(
            oapi_codegen::check_package(&output, &package).is_err(),
            "checking must refuse `{escape}` too",
        );
    }

    assert_eq!(
        std::fs::read_to_string(&victim).unwrap_or_else(|err| panic!("reading failed: {err}")),
        "// mine, not the generator's\n",
        "the file outside the companion directory must be untouched",
    );
    assert!(!output.exists(), "a refused run must write nothing");
}

#[test]
fn writing_a_split_package_to_an_extensionless_path_is_refused() {
    let dir = TestDir::new("write_no_extension");
    let package = package_for(&dir.spec(), &full_config(), &dir.output());
    // `generate_package` rejects this path, but `write_package` is public and
    // must not half-write a package it cannot lay out.
    let output = dir.join("restapi");

    let error = oapi_codegen::write_package(&output, &package)
        .expect_err("a package with children needs a companion directory beside the root");
    assert!(
        matches!(&error, oapi_codegen::Error::UnsplittableOutput { .. }),
        "expected `UnsplittableOutput`, got: {error}",
    );
    assert!(!output.exists(), "nothing may be written");
}

#[test]
fn a_pipe_in_the_companion_directory_is_refused_without_reading_it() {
    #[cfg(unix)]
    {
        let dir = TestDir::new("fifo");
        let output = dir.output();
        let package = package_for(&dir.spec(), &full_config(), &output);
        oapi_codegen::write_package(&output, &package).unwrap_or_else(|err| panic!("writing failed: {err}"));

        // Opening a pipe with no writer blocks for ever. The generator writes
        // only regular files, so the walk must refuse this without reading it.
        let fifo = dir.join("restapi/pipe.rs");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap_or_else(|err| panic!("running mkfifo failed: {err}"));
        assert!(status.success(), "mkfifo should create `{}`", fifo.display());

        let error = oapi_codegen::check_package(&output, &package).expect_err("checking must refuse a pipe");
        assert!(
            matches!(&error, oapi_codegen::Error::UnownedOutput { path } if *path == fifo.display().to_string()),
            "expected `UnownedOutput` for `{}`, got: {error}",
            fifo.display(),
        );
    }
}
