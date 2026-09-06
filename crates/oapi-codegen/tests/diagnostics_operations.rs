use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

const CONFIG: &str = "package: demo\ngenerate:\n  models: true\n  std-http-server: true\n";

const SPEC: &str = "\
openapi: 3.0.3
info:
  title: Demo
  version: 1.0.0
paths:
  /items:
    post:
      operationId: createItem
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: string
      responses:
        '200':
          description: The item
          content:
            application/json:
              schema:
                type: string
";

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let elapsed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|err| panic!("the system clock must be after the Unix epoch: {err}"));
        let path = std::env::temp_dir().join(format!(
            "oapi-codegen-diagnostics-operations-{name}-{}-{}",
            std::process::id(),
            elapsed.as_nanos()
        ));
        std::fs::create_dir_all(path.join("generated"))
            .unwrap_or_else(|err| panic!("cannot create the test directory: {err}"));
        let dir = Self { path };
        dir.write("config.yaml", CONFIG);
        return dir;
    }

    fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.path.join(name), contents)
            .unwrap_or_else(|err| panic!("cannot write the test input: {err}"));
    }

    fn run(&self) -> Output {
        return Command::new(env!("CARGO_BIN_EXE_oapi-codegen"))
            .arg("--config-file")
            .arg(self.path.join("config.yaml"))
            .arg("--output-file")
            .arg(self.path.join("generated/output.rs"))
            .arg(self.path.join("spec.yaml"))
            .output()
            .unwrap_or_else(|err| panic!("cannot run the generator: {err}"));
    }

    fn generated(&self) -> BTreeMap<PathBuf, String> {
        return read_tree(&self.path.join("generated"));
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn read_tree(path: &Path) -> BTreeMap<PathBuf, String> {
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(path).unwrap_or_else(|err| panic!("cannot read the generated directory: {err}")) {
        let path = entry
            .unwrap_or_else(|err| panic!("cannot read the generated entry: {err}"))
            .path();
        if path.is_dir() {
            files.extend(read_tree(&path));
        } else {
            let contents =
                std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("cannot read the generated file: {err}"));
            files.insert(path, contents);
        }
    }
    return files;
}

fn successful_stderr(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{stderr}");
    return stderr;
}

#[test]
fn mixed_unsupported_media_warns_without_changing_generated_bodies() {
    let dir = TestDir::new("mixed-media");
    dir.write("spec.yaml", SPEC);
    successful_stderr(&dir.run());
    let original = dir.generated();
    let spec = SPEC
        .replace(
            "\n          application/json:",
            "\n          application/xml:\n            schema:\n              type: integer\n          application/json:",
        )
        .replace(
            "\n            application/json:",
            "\n            application/octet-stream:\n              schema:\n                type: integer\n            application/json:",
        );
    dir.write("spec.yaml", &spec);
    let stderr = successful_stderr(&dir.run());
    assert!(
        stderr.contains("unsupported media type `application/xml` is ignored"),
        "{stderr}"
    );
    assert!(
        stderr.contains("unsupported media type `application/octet-stream` is ignored"),
        "{stderr}"
    );
    assert!(stderr.contains("post /items request body"), "{stderr}");
    assert!(stderr.contains("post /items `200` response"), "{stderr}");
    assert_eq!(dir.generated(), original);
}

#[test]
fn duplicate_body_kinds_warn_and_keep_the_first_representation() {
    let dir = TestDir::new("duplicate-media");
    let first = SPEC.replace("application/json:", "application/vnd.first+json:");
    dir.write("spec.yaml", &first);
    successful_stderr(&dir.run());
    let original = dir.generated();
    let spec = first
        .replace(
            "              type: string\n      responses:",
            "              type: string\n          application/json:\n            schema:\n              type: integer\n      responses:",
        )
        + "            application/json:\n              schema:\n                type: integer\n";
    dir.write("spec.yaml", &spec);
    let stderr = successful_stderr(&dir.run());
    let message = "media type `application/json` is ignored because `application/vnd.first+json` is the first representation of the same body kind";
    assert_eq!(stderr.matches(message).count(), 2, "{stderr}");
    assert_eq!(dir.generated(), original);
}

#[test]
fn resolved_response_media_warns_with_the_origin_file() {
    let dir = TestDir::new("referenced-media");
    dir.write(
        "spec.yaml",
        "openapi: 3.0.3\ninfo:\n  title: Demo\n  version: 1.0.0\npaths:\n  /items:\n    get:\n      responses:\n        '200':\n          $ref: 'responses.yaml#/components/responses/Item'\n",
    );
    let response = "openapi: 3.0.3\ninfo:\n  title: Responses\n  version: 1.0.0\npaths: {}\ncomponents:\n  responses:\n    Item:\n      description: The item\n      content:\n        application/json:\n          schema:\n            type: string\n";
    dir.write("responses.yaml", response);
    successful_stderr(&dir.run());
    let original = dir.generated();
    dir.write(
        "responses.yaml",
        &format!("{response}        application/xml:\n          schema:\n            type: integer\n        application/vnd.second+json:\n          schema:\n            type: integer\n"),
    );
    let stderr = successful_stderr(&dir.run());
    assert!(stderr.contains("get /items `200` response"), "{stderr}");
    assert!(stderr.contains("resolved from `responses.yaml`"), "{stderr}");
    assert!(
        stderr.contains("unsupported media type `application/xml` is ignored"),
        "{stderr}"
    );
    assert!(
        stderr.contains("media type `application/vnd.second+json` is ignored"),
        "{stderr}"
    );
    assert_eq!(dir.generated(), original);
}

#[test]
fn scalar_styles_warn_but_scalar_explode_settings_do_not() {
    let dir = TestDir::new("scalar-styles");
    let spec = "openapi: 3.0.3\ninfo:\n  title: Demo\n  version: 1.0.0\npaths:\n  /items/{id}:\n    get:\n      parameters:\n        - in: path\n          name: id\n          required: true\n          style: simple\n          schema:\n            type: string\n        - in: query\n          name: search\n          style: form\n          schema:\n            type: string\n        - in: header\n          name: X-Item\n          style: simple\n          explode: true\n          schema:\n            type: string\n        - in: cookie\n          name: item\n          style: form\n          explode: false\n          schema:\n            type: string\n      responses:\n        '204':\n          description: No content\n";
    dir.write("spec.yaml", spec);
    let stderr = successful_stderr(&dir.run());
    assert!(!stderr.contains("unsupported scalar style"), "{stderr}");
    assert!(!stderr.contains("unsupported style"), "{stderr}");
    assert!(!stderr.contains("explode"), "{stderr}");
    let original = dir.generated();
    for style in ["label", "matrix"] {
        let changed = spec.replacen("style: simple", &format!("style: {style}"), 1).replacen(
            "style: form",
            "style: spaceDelimited",
            1,
        );
        dir.write("spec.yaml", &changed);
        let stderr = successful_stderr(&dir.run());
        assert!(
            stderr.contains("path parameter `id` uses unsupported style"),
            "{stderr}"
        );
        assert!(
            stderr.contains("query parameter `search` uses unsupported scalar style"),
            "{stderr}"
        );
        assert_eq!(dir.generated(), original);
    }
}

#[test]
fn unsupported_only_content_still_fails_without_replacing_output() {
    let dir = TestDir::new("unsupported-only");
    dir.write("spec.yaml", SPEC);
    successful_stderr(&dir.run());
    let original = dir.generated();
    for indentation in ["          ", "            "] {
        let changed = SPEC.replace(
            &format!("\n{indentation}application/json:"),
            &format!("\n{indentation}application/xml:"),
        );
        dir.write("spec.yaml", &changed);
        let output = dir.run();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(stderr.contains("application/xml"), "{stderr}");
        assert!(
            !stderr.contains("unsupported media type `application/xml` is ignored"),
            "{stderr}"
        );
        assert_eq!(dir.generated(), original);
    }
}

#[test]
fn resolved_parameter_style_warns_with_the_origin_file() {
    let dir = TestDir::new("referenced-parameter");
    dir.write(
        "spec.yaml",
        "openapi: 3.0.3\ninfo:\n  title: Demo\n  version: 1.0.0\npaths:\n  /items/{id}:\n    get:\n      parameters:\n        - $ref: 'parameters.yaml#/components/parameters/Id'\n      responses:\n        '204':\n          description: No content\n",
    );
    let parameter = "openapi: 3.0.3\ninfo:\n  title: Parameters\n  version: 1.0.0\npaths: {}\ncomponents:\n  parameters:\n    Id:\n      in: path\n      name: id\n      required: true\n      style: simple\n      schema:\n        type: string\n";
    dir.write("parameters.yaml", parameter);
    successful_stderr(&dir.run());
    let original = dir.generated();
    dir.write("parameters.yaml", &parameter.replace("style: simple", "style: matrix"));
    let stderr = successful_stderr(&dir.run());
    assert!(stderr.contains("get /items/{id}"), "{stderr}");
    assert!(
        stderr.contains("path parameter `id` uses unsupported style"),
        "{stderr}"
    );
    assert!(stderr.contains("resolved from `parameters.yaml`"), "{stderr}");
    assert_eq!(dir.generated(), original);
}

#[test]
fn scalar_parameter_enums_warn_without_changing_generated_types() {
    for location in ["path", "query", "header", "cookie"] {
        for (kind, values) in [("string", "[one, two]"), ("integer", "[1, 2]")] {
            let dir = TestDir::new(&format!("enum-{location}-{kind}"));
            let path = if location == "path" { "/items/{value}" } else { "/items" };
            let spec = format!(
                "openapi: 3.0.3\ninfo:\n  title: Demo\n  version: 1.0.0\npaths:\n  {path}:\n    get:\n      parameters:\n        - in: {location}\n          name: value\n          required: true\n          schema:\n            type: {kind}\n      responses:\n        '204':\n          description: No content\n"
            );
            dir.write("spec.yaml", &spec);
            let stderr = successful_stderr(&dir.run());
            assert!(!stderr.contains("the declared `enum` is ignored"), "{stderr}");
            let original = dir.generated();
            dir.write(
                "spec.yaml",
                &spec.replace(
                    &format!("type: {kind}"),
                    &format!("type: {kind}\n            enum: {values}"),
                ),
            );
            let stderr = successful_stderr(&dir.run());
            assert!(stderr.contains(&format!("{location} parameter `value`")), "{stderr}");
            assert_eq!(stderr.matches("the declared `enum` is ignored").count(), 1, "{stderr}");
            assert_eq!(dir.generated(), original);
        }
    }
}

#[test]
fn resolved_parameter_schema_enums_warn_for_scalars_and_array_items() {
    for array in [false, true] {
        let dir = TestDir::new(if array { "enum-ref-array" } else { "enum-ref-scalar" });
        dir.write(
            "spec.yaml",
            "openapi: 3.0.3\ninfo:\n  title: Demo\n  version: 1.0.0\npaths:\n  /items:\n    get:\n      parameters:\n        - $ref: 'parameters.yaml#/components/parameters/Value'\n      responses:\n        '204':\n          description: No content\n",
        );
        let schema = if array {
            "        type: array\n        items:\n          $ref: '#/components/schemas/Value'"
        } else {
            "        $ref: '#/components/schemas/Value'"
        };
        let parameter = format!(
            "openapi: 3.0.3\ninfo:\n  title: Parameters\n  version: 1.0.0\npaths: {{}}\ncomponents:\n  parameters:\n    Value:\n      in: query\n      name: value\n      schema:\n{schema}\n  schemas:\n    Value:\n      type: string\n"
        );
        dir.write("parameters.yaml", &parameter);
        let stderr = successful_stderr(&dir.run());
        assert!(!stderr.contains("the declared `enum` is ignored"), "{stderr}");
        let original = dir.generated();
        dir.write("parameters.yaml", &format!("{parameter}      enum: [one, two]\n"));
        let stderr = successful_stderr(&dir.run());
        assert!(stderr.contains("get /items query parameter `value`"), "{stderr}");
        assert!(stderr.contains("resolved from `parameters.yaml`"), "{stderr}");
        assert_eq!(stderr.matches("the declared `enum` is ignored").count(), 1, "{stderr}");
        assert_eq!(dir.generated(), original);
    }
}
