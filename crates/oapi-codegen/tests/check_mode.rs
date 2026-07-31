//! Exit codes and file effects of `--check`.
//!
//! A consumer gates a build on the exit code, so the code itself is the contract
//! and a unit test of the comparison does not cover it. These cases run the real
//! binary and read the file system afterwards.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

/// A run that found the output file current, or that wrote it.
const SUCCESS: i32 = 0;
/// A run that found drift, or that failed to generate.
const FAILURE: i32 = 1;
/// A usage error, which `clap` reports and the generator never reaches.
const USAGE_ERROR: i32 = 2;

/// A minimal spec with one component schema.
const SPEC: &str = "\
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

/// The same spec with a second property, so its output differs from `SPEC`.
const SPEC_CHANGED: &str = "\
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
        colour:
          type: string
";

/// A models-only configuration. The output path comes from `--output-file`.
const CONFIG: &str = "\
package: demo
generate:
  models: true
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
            "oapi-codegen-check-mode-{test_name}-{}-{}",
            std::process::id(),
            elapsed.as_nanos(),
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).unwrap_or_else(|err| panic!("creating `{}` failed: {err}", path.display()));
        let dir = Self { path };
        dir.write("config.yaml", CONFIG);
        dir.write("spec.yaml", SPEC);
        return dir;
    }

    fn write(&self, file: &str, contents: &str) {
        let path = self.path.join(file);
        std::fs::write(&path, contents).unwrap_or_else(|err| panic!("writing `{}` failed: {err}", path.display()));
    }

    fn join(&self, file: &str) -> PathBuf {
        return self.path.join(file);
    }

    /// The generated file this directory's runs compare against.
    fn output(&self) -> PathBuf {
        return self.join("output.rs");
    }

    /// Run the generator, with `--check` when `check` is set.
    fn run(&self, check: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_oapi-codegen"));
        command
            .arg("--config-file")
            .arg(self.join("config.yaml"))
            .arg("--output-file")
            .arg(self.output());
        if check {
            command.arg("--check");
        }
        command.arg(self.join("spec.yaml"));
        return run_command(&mut command);
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Run `command` to completion.
fn run_command(command: &mut Command) -> Output {
    return command
        .output()
        .unwrap_or_else(|err| panic!("running the generator failed: {err}"));
}

/// The exit code of `output`, which every supported platform reports.
fn code(output: &Output) -> i32 {
    return output
        .status
        .code()
        .unwrap_or_else(|| panic!("the generator exited with a signal and no code"));
}

/// The stderr of `output` as text. Every report goes to stderr.
fn stderr(output: &Output) -> String {
    return String::from_utf8_lossy(&output.stderr).into_owned();
}

/// Read `path`, which every case here has already generated.
fn read(path: &Path) -> String {
    return std::fs::read_to_string(path).unwrap_or_else(|err| panic!("reading `{}` failed: {err}", path.display()));
}

#[test]
fn check_with_no_output_file_fails_and_writes_nothing() {
    let dir = TestDir::new("absent");
    let output = dir.run(true);
    assert_eq!(code(&output), FAILURE, "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("does not exist"),
        "the report must name the absent file: {}",
        stderr(&output)
    );
    // `--check` must not create the file it compares against.
    assert!(!dir.output().exists(), "`--check` created the output file");
}

#[test]
fn check_with_an_up_to_date_output_file_passes() {
    let dir = TestDir::new("passes");
    assert_eq!(code(&dir.run(false)), SUCCESS, "the write must succeed first");
    let output = dir.run(true);
    assert_eq!(code(&output), SUCCESS, "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("up to date"),
        "the report must say the file is current: {}",
        stderr(&output)
    );
}

#[test]
fn check_with_a_stale_output_file_fails_and_leaves_it_alone() {
    let dir = TestDir::new("stale");
    assert_eq!(code(&dir.run(false)), SUCCESS, "the write must succeed first");
    let before = read(&dir.output());

    // A spec change with no regenerated output is the case the gate catches.
    dir.write("spec.yaml", SPEC_CHANGED);
    let output = dir.run(true);
    assert_eq!(code(&output), FAILURE, "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("out of date"),
        "the report must say the file is stale: {}",
        stderr(&output)
    );

    let after = read(&dir.output());
    assert_eq!(after, before, "`--check` must not update the output file");
    assert!(!after.contains("colour"), "`--check` wrote the new property");
}

#[test]
fn a_run_after_a_failed_check_makes_the_check_pass() {
    let dir = TestDir::new("recovers");
    assert_eq!(
        code(&dir.run(true)),
        FAILURE,
        "the file is absent, so the check must fail"
    );
    assert_eq!(code(&dir.run(false)), SUCCESS, "the write must succeed");
    assert_eq!(code(&dir.run(true)), SUCCESS, "the check must pass after the write");
}

#[test]
fn check_rejects_install_deps() {
    let dir = TestDir::new("conflict");
    // `--check` writes nothing, so it adds no crate to a manifest. Accepting the
    // two flags together would promise an install that cannot happen.
    let mut command = Command::new(env!("CARGO_BIN_EXE_oapi-codegen"));
    command
        .arg("--config-file")
        .arg(dir.join("config.yaml"))
        .arg("--output-file")
        .arg(dir.output())
        .arg("--check")
        .arg("--install-deps")
        .arg(dir.join("spec.yaml"));
    let output = run_command(&mut command);
    assert_eq!(code(&output), USAGE_ERROR, "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("cannot be used with"),
        "clap must report the conflict: {}",
        stderr(&output)
    );
}

#[test]
fn check_reports_a_spec_error_and_not_drift() {
    let dir = TestDir::new("spec-error");
    // A spec the generator rejects must give the generator's own report. A gate
    // that showed this as drift would send the reader to regenerate, which
    // cannot succeed either.
    dir.write("spec.yaml", "openapi: 3.0.3\nthis is not a spec\n");
    let output = dir.run(true);
    assert_eq!(code(&output), FAILURE, "stderr: {}", stderr(&output));
    let report = stderr(&output);
    assert!(
        !report.contains("out of date") && !report.contains("does not exist"),
        "a spec error must not report as drift: {report}"
    );
}
