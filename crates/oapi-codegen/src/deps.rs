//! Computing the external crate dependencies the generated code requires.
//!
//! Unlike Go — where `go mod tidy` resolves imports from the generated source
//! automatically — Cargo never infers dependencies from `use` paths, and a path
//! like `http::StatusCode` reveals neither the crate version nor the Cargo
//! features a consumer must enable. The generator is the only component that
//! knows exactly what it emitted, so it reports the crates (with versions and
//! features) a consumer should add to `Cargo.toml`.
//!
//! The set is derived by scanning the generated source for the crate-root paths
//! and method calls the emitters produce. Deriving it from the actual output
//! (rather than re-deriving from the IR) keeps this report automatically in step
//! with the emitters: if they stop or start referencing a crate, the report
//! follows without a parallel rule set to maintain.

/// This crate's own manifest, embedded at compile time so the versions the
/// report recommends always match the versions the generated code is compiled
/// and tested against here (see [`manifest_version`]).
const MANIFEST: &str = include_str!("../Cargo.toml");

/// The version requirement declared for `crate_name` in this crate's own
/// `[dependencies]` or `[dev-dependencies]`.
///
/// Deriving the recommended version from our manifest (rather than a hardcoded
/// constant) keeps the report in lockstep with the versions the generated
/// goldens actually compile against, so a dependency bump here updates the
/// report automatically. Every crate the report can name is a (dev-)dependency
/// of this crate — enforced by `reported_crates_have_manifest_versions` — so an
/// absent entry is a programming error, not runtime input.
fn manifest_version(crate_name: &str) -> &'static str {
    for line in MANIFEST.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(crate_name) else {
            continue;
        };
        // Require a token boundary so `serde` does not match `serde_json`.
        if !rest.starts_with([' ', '\t', '=']) {
            continue;
        }
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        // `name = "x"` gives the version directly; `name = { version = "x", .. }`
        // needs the `version` key located first.
        let scan = match value.strip_prefix('{') {
            Some(table) => match table.find("version") {
                Some(index) => &table[index..],
                None => continue,
            },
            None => value,
        };
        if let Some(open) = scan.find('"') {
            let after = &scan[open + 1..];
            if let Some(close) = after.find('"') {
                return &after[..close];
            }
        }
    }
    panic!(
        "crate `{crate_name}` is not a declared dependency of oapi-codegen; cannot determine its version for the dependency report"
    );
}

/// A crate the generated code references, with the version requirement and Cargo
/// features a consumer should declare in `Cargo.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The crates.io crate name (e.g. `axum-extra`).
    pub name: &'static str,
    /// Recommended version requirement (major series).
    pub version: &'static str,
    /// Whether the crate's default features are needed.
    pub default_features: bool,
    /// Cargo features the generated code relies on, in a stable order.
    pub features: Vec<&'static str>,
}

impl Dependency {
    /// Render the `Cargo.toml` `[dependencies]` entry for this crate.
    ///
    /// A crate needing neither features nor a `default-features` change renders
    /// as the short `name = "version"` form; otherwise the inline-table form.
    pub fn toml(&self) -> String {
        if self.default_features && self.features.is_empty() {
            return format!("{} = \"{}\"", self.name, self.version);
        }
        let mut parts = vec![format!("version = \"{}\"", self.version)];
        if !self.default_features {
            parts.push("default-features = false".to_owned());
        }
        if !self.features.is_empty() {
            let features = self
                .features
                .iter()
                .map(|feature| return format!("\"{feature}\""))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("features = [{features}]"));
        }
        return format!("{} = {{ {} }}", self.name, parts.join(", "));
    }

    /// Render the equivalent `cargo add` command.
    pub fn cargo_add(&self) -> String {
        let mut command = format!("cargo add {}@{}", self.name, self.version);
        if !self.default_features {
            command.push_str(" --no-default-features");
        }
        if !self.features.is_empty() {
            command.push_str(&format!(" --features {}", self.features.join(",")));
        }
        return command;
    }

    /// The `cargo` arguments that add this dependency, for `std::process::Command`.
    pub fn cargo_add_args(&self) -> Vec<String> {
        let mut args = vec!["add".to_owned(), format!("{}@{}", self.name, self.version)];
        if !self.default_features {
            args.push("--no-default-features".to_owned());
        }
        if !self.features.is_empty() {
            args.push("--features".to_owned());
            args.push(self.features.join(","));
        }
        return args;
    }
}

/// Inspect generated `code` and return the external crates it references, in a
/// stable order (shared model crates, then server crates, then client crates).
pub fn required_dependencies(code: &str) -> Vec<Dependency> {
    let has = |needle: &str| return code.contains(needle);
    let mut deps = Vec::new();

    if has("serde::Serialize") || has("serde::Deserialize") {
        deps.push(with_features("serde", true, vec!["derive"]));
    }
    if has("serde_json::") {
        deps.push(plain("serde_json"));
    }
    if has("chrono::") {
        deps.push(with_features("chrono", true, vec!["serde"]));
    }
    if has("uuid::") {
        deps.push(with_features("uuid", true, vec!["serde"]));
    }
    if has("http::") {
        deps.push(plain("http"));
    }
    if has("axum::") {
        let mut features = Vec::new();
        if has("axum::extract::Multipart") {
            features.push("multipart");
        }
        deps.push(with_features("axum", true, features));
    }
    if has("axum_extra::") {
        let mut features = Vec::new();
        if has("axum_extra::extract::Query") {
            features.push("query");
        }
        if has("axum_extra::extract::CookieJar") {
            features.push("cookie");
        }
        deps.push(with_features("axum-extra", true, features));
    }
    if has("reqwest::") {
        let mut features = Vec::new();
        if has("reqwest::blocking") {
            features.push("blocking");
        }
        if has(".json(") {
            features.push("json");
        }
        if has(".form(") {
            features.push("form");
        }
        if has(".query(") {
            features.push("query");
        }
        if has(".multipart(") || has("reqwest::blocking::multipart") {
            features.push("multipart");
        }
        deps.push(with_features("reqwest", false, features));
    }
    if has("percent_encoding::") {
        deps.push(plain("percent-encoding"));
    }
    if has("serde_urlencoded::") {
        deps.push(plain("serde_urlencoded"));
    }

    return deps;
}

/// A dependency whose version is taken from this crate's manifest, needing
/// default features and no extra features.
fn plain(name: &'static str) -> Dependency {
    return with_features(name, true, Vec::new());
}

/// A dependency whose version is taken from this crate's manifest, with the
/// given default-features flag and feature list.
fn with_features(name: &'static str, default_features: bool, features: Vec<&'static str>) -> Dependency {
    return Dependency {
        name,
        version: manifest_version(name),
        default_features,
        features,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_renders_short_and_table_forms() {
        assert_eq!(
            Dependency {
                name: "http",
                version: "1",
                default_features: true,
                features: vec![],
            }
            .toml(),
            "http = \"1\""
        );
        assert_eq!(
            Dependency {
                name: "serde",
                version: "1",
                default_features: true,
                features: vec!["derive"],
            }
            .toml(),
            "serde = { version = \"1\", features = [\"derive\"] }"
        );
        assert_eq!(
            Dependency {
                name: "reqwest",
                version: "0.13",
                default_features: false,
                features: vec!["blocking", "json"],
            }
            .toml(),
            "reqwest = { version = \"0.13\", default-features = false, features = [\"blocking\", \"json\"] }"
        );
    }

    #[test]
    fn cargo_add_renders_flags() {
        assert_eq!(
            Dependency {
                name: "http",
                version: "1",
                default_features: true,
                features: vec![],
            }
            .cargo_add(),
            "cargo add http@1"
        );
        assert_eq!(
            Dependency {
                name: "reqwest",
                version: "0.13",
                default_features: false,
                features: vec!["blocking", "json"],
            }
            .cargo_add(),
            "cargo add reqwest@0.13 --no-default-features --features blocking,json"
        );
    }

    #[test]
    fn cargo_add_args_split_for_process_execution() {
        assert_eq!(
            Dependency {
                name: "http",
                version: "1",
                default_features: true,
                features: vec![],
            }
            .cargo_add_args(),
            vec!["add", "http@1"]
        );
        assert_eq!(
            Dependency {
                name: "reqwest",
                version: "0.13",
                default_features: false,
                features: vec!["blocking", "json"],
            }
            .cargo_add_args(),
            vec![
                "add",
                "reqwest@0.13",
                "--no-default-features",
                "--features",
                "blocking,json"
            ]
        );
    }

    #[test]
    fn versions_come_from_the_manifest_not_hardcoded() {
        // The bare `name = "x"` form and the `{ version = "x", .. }` table form
        // are both read from this crate's own Cargo.toml.
        assert_eq!(manifest_version("http"), extract_manifest_version("http"));
        assert_eq!(manifest_version("axum"), extract_manifest_version("axum"));
        assert!(!manifest_version("serde_urlencoded").is_empty());
    }

    #[test]
    fn serde_prefix_does_not_match_serde_json_or_urlencoded() {
        // `serde` must resolve to the `serde` line, not `serde_json`/`serde_urlencoded`.
        assert_eq!(manifest_version("serde"), extract_manifest_version("serde"));
        assert_ne!(manifest_version("serde"), manifest_version("serde_json"));
    }

    #[test]
    fn every_reportable_crate_has_a_manifest_version() {
        // A code blob that trips every detection branch; if any reported crate
        // lacked a manifest entry, `manifest_version` would panic here.
        let code = "\
            serde::Serialize serde_json::Value chrono::DateTime uuid::Uuid http::StatusCode \
            axum::extract::Multipart axum_extra::extract::Query axum_extra::extract::CookieJar \
            reqwest::blocking::multipart .json( .form( .query( percent_encoding::utf8 serde_urlencoded::from_str";
        for dep in required_dependencies(code) {
            assert!(!dep.version.is_empty(), "{} has an empty version", dep.name);
        }
    }

    /// Independent re-implementation used only to cross-check [`manifest_version`]:
    /// find `name` in the embedded manifest and return the first quoted string
    /// after the `version` key (or the bare value).
    fn extract_manifest_version(name: &str) -> String {
        for line in MANIFEST.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix(name)
                && rest.starts_with([' ', '\t', '='])
            {
                let quoted: Vec<&str> = line.split('"').collect();
                // `name = "x"` -> [.., "x", ..]; `{ version = "x", features = [..] }`
                // -> the version is the first quoted token.
                if quoted.len() >= 2 {
                    return quoted[1].to_owned();
                }
            }
        }
        panic!("`{name}` not found in manifest");
    }

    #[test]
    fn detects_server_stack_from_generated_paths() {
        let code = "axum::Json axum::extract::Multipart axum_extra::extract::Query http::StatusCode serde::Serialize serde_json::Value";
        let deps = required_dependencies(code);
        let names: Vec<&str> = deps.iter().map(|dep| return dep.name).collect();
        assert_eq!(names, vec!["serde", "serde_json", "http", "axum", "axum-extra"]);
        let axum = deps.iter().find(|dep| return dep.name == "axum").expect("axum present");
        assert_eq!(axum.features, vec!["multipart"]);
        let extra = deps
            .iter()
            .find(|dep| return dep.name == "axum-extra")
            .expect("axum-extra present");
        assert_eq!(extra.features, vec!["query"]);
    }

    #[test]
    fn detects_client_stack_from_generated_paths() {
        let code = "reqwest::blocking::Client request.json(&body) percent_encoding::utf8 serde::Deserialize";
        let deps = required_dependencies(code);
        let reqwest = deps
            .iter()
            .find(|dep| return dep.name == "reqwest")
            .expect("reqwest present");
        assert!(!reqwest.default_features);
        assert_eq!(reqwest.features, vec!["blocking", "json"]);
        assert!(deps.iter().any(|dep| return dep.name == "percent-encoding"));
    }

    #[test]
    fn axum_marker_does_not_match_axum_extra() {
        let deps = required_dependencies("axum_extra::extract::CookieJar");
        assert!(
            !deps.iter().any(|dep| return dep.name == "axum"),
            "`axum_extra::` must not be mistaken for the `axum` crate",
        );
        let extra = deps
            .iter()
            .find(|dep| return dep.name == "axum-extra")
            .expect("axum-extra present");
        assert_eq!(extra.features, vec!["cookie"]);
    }

    #[test]
    fn no_dependencies_for_dependency_free_output() {
        assert!(required_dependencies("pub const SERVER_URL: &str = \"https://x\";").is_empty());
    }
}
