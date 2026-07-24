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

/// Recommended version requirements, pinned to the major series the generated
/// code is written against (and compiled against in this crate's tests).
const V_SERDE: &str = "1";
const V_SERDE_JSON: &str = "1";
const V_CHRONO: &str = "0.4";
const V_UUID: &str = "1";
const V_HTTP: &str = "1";
const V_AXUM: &str = "0.8";
const V_AXUM_EXTRA: &str = "0.12";
const V_REQWEST: &str = "0.13";
const V_PERCENT_ENCODING: &str = "2";
const V_SERDE_URLENCODED: &str = "0.7";

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
        deps.push(Dependency {
            name: "serde",
            version: V_SERDE,
            default_features: true,
            features: vec!["derive"],
        });
    }
    if has("serde_json::") {
        deps.push(plain("serde_json", V_SERDE_JSON));
    }
    if has("chrono::") {
        deps.push(Dependency {
            name: "chrono",
            version: V_CHRONO,
            default_features: true,
            features: vec!["serde"],
        });
    }
    if has("uuid::") {
        deps.push(Dependency {
            name: "uuid",
            version: V_UUID,
            default_features: true,
            features: vec!["serde"],
        });
    }
    if has("http::") {
        deps.push(plain("http", V_HTTP));
    }
    if has("axum::") {
        let mut features = Vec::new();
        if has("axum::extract::Multipart") {
            features.push("multipart");
        }
        deps.push(Dependency {
            name: "axum",
            version: V_AXUM,
            default_features: true,
            features,
        });
    }
    if has("axum_extra::") {
        let mut features = Vec::new();
        if has("axum_extra::extract::Query") {
            features.push("query");
        }
        if has("axum_extra::extract::CookieJar") {
            features.push("cookie");
        }
        deps.push(Dependency {
            name: "axum-extra",
            version: V_AXUM_EXTRA,
            default_features: true,
            features,
        });
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
        deps.push(Dependency {
            name: "reqwest",
            version: V_REQWEST,
            default_features: false,
            features,
        });
    }
    if has("percent_encoding::") {
        deps.push(plain("percent-encoding", V_PERCENT_ENCODING));
    }
    if has("serde_urlencoded::") {
        deps.push(plain("serde_urlencoded", V_SERDE_URLENCODED));
    }

    return deps;
}

/// A dependency needing default features and no extra features.
fn plain(name: &'static str, version: &'static str) -> Dependency {
    return Dependency {
        name,
        version,
        default_features: true,
        features: Vec::new(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_renders_short_and_table_forms() {
        assert_eq!(plain("http", "1").toml(), "http = \"1\"");
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
        assert_eq!(plain("http", "1").cargo_add(), "cargo add http@1");
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
        assert_eq!(plain("http", "1").cargo_add_args(), vec!["add", "http@1"]);
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
