//! Lowering the spec's `servers:` block into the [`ServerUrls`] IR.
//!
//! Each declared server becomes either a constant (no `{placeholder}` in the
//! URL) or a builder function that substitutes its variables. Enum-constrained
//! variables additionally produce a dedicated Rust `enum` type so callers pass a
//! validated value rather than a bare string.
//!
//! This mirrors `oapi-codegen`'s `server-urls` generator, including its handling
//! of declared-but-unused variables (skipped) and URL placeholders with no
//! matching variable (emitted as free-form `&str` parameters).

use std::collections::HashMap;
use std::collections::HashSet;

use openapiv3::Server;
use openapiv3::ServerVariable;

use crate::error::Error;
use crate::error::Result;
use crate::ir::ServerUrl;
use crate::ir::ServerUrlBuilder;
use crate::ir::ServerUrlConst;
use crate::ir::ServerUrlEnum;
use crate::ir::ServerUrlEnumVariant;
use crate::ir::ServerUrlParam;
use crate::ir::ServerUrlParamType;
use crate::ir::ServerUrls;
use crate::loader::Spec;
use crate::naming::Case;
use crate::naming::X_RUST_NAME;
use crate::naming::deconflict_ident;
use crate::naming::to_ident;

/// Prefix applied to server identifiers derived from a description or URL, so a
/// bare description like `Production` becomes `ServerUrlProduction`.
const SERVER_PREFIX: &str = "server url";

/// Lower the spec's `servers:` into the [`ServerUrls`] IR, or `None` when the
/// document declares no servers.
pub fn lower_server_urls(spec: &Spec) -> Result<Option<ServerUrls>> {
    let servers = spec.servers();
    if servers.is_empty() {
        return Ok(None);
    }
    let mut used_names: HashMap<String, usize> = HashMap::new();
    let mut enums = Vec::new();
    let mut lowered = Vec::new();
    for server in servers {
        let seed = deconflict(name_seed(server)?, &mut used_names);
        lowered.push(lower_server(server, &seed, &mut enums)?);
    }
    return Ok(Some(ServerUrls {
        enums,
        servers: lowered,
    }));
}

/// Lower a single server into a constant or builder, pushing any enum types it
/// needs onto `enums`.
fn lower_server(server: &Server, seed: &str, enums: &mut Vec<ServerUrlEnum>) -> Result<ServerUrl> {
    let label = label_of(server)?;
    let doc = Some(doc_of(server, &label));
    let placeholders = placeholders(&server.url);
    if placeholders.is_empty() {
        return Ok(ServerUrl::Const(ServerUrlConst {
            name: to_ident(seed, Case::ScreamingSnake),
            doc,
            url: server.url.clone(),
        }));
    }
    let variables = server.variables.as_ref();
    let mut params = Vec::with_capacity(placeholders.len());
    for placeholder in &placeholders {
        let variable = variables.and_then(|vars| return vars.get(placeholder));
        let ty = param_type(server, seed, &label, placeholder, variable, enums)?;
        params.push(ServerUrlParam {
            ident: to_ident(placeholder, Case::Snake),
            placeholder: placeholder.clone(),
            ty,
        });
    }
    return Ok(ServerUrl::Builder(ServerUrlBuilder {
        name: to_ident(seed, Case::Snake),
        doc,
        url_template: server.url.clone(),
        params,
    }));
}

/// Determine a placeholder's parameter type, synthesising an enum type for an
/// enum-constrained variable.
fn param_type(
    server: &Server,
    seed: &str,
    label: &str,
    placeholder: &str,
    variable: Option<&ServerVariable>,
    enums: &mut Vec<ServerUrlEnum>,
) -> Result<ServerUrlParamType> {
    let Some(variable) = variable else {
        return Ok(ServerUrlParamType::Str);
    };
    if variable.enumeration.is_empty() {
        return Ok(ServerUrlParamType::Str);
    }
    let enom = build_enum(server, seed, label, placeholder, variable)?;
    let name = enom.name.clone();
    enums.push(enom);
    return Ok(ServerUrlParamType::Enum(name));
}

/// Build the enum type for an enum-constrained server variable, validating that
/// its declared `default` (if any) is one of the enum values.
fn build_enum(
    server: &Server,
    seed: &str,
    label: &str,
    placeholder: &str,
    variable: &ServerVariable,
) -> Result<ServerUrlEnum> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut variants = Vec::with_capacity(variable.enumeration.len());
    let mut default = None;
    for value in &variable.enumeration {
        let ident = deconflict_ident(to_ident(value, Case::Pascal), &mut seen);
        if !variable.default.is_empty() && value == &variable.default {
            default = Some(ident.clone());
        }
        variants.push(ServerUrlEnumVariant {
            name: ident,
            value: value.clone(),
        });
    }
    if !variable.default.is_empty() && default.is_none() {
        return Err(Error::UnsupportedSchema {
            path: format!("servers[{}].variables.{placeholder}", server.url),
            reason: format!(
                "default value `{}` is not one of the declared enum values",
                variable.default
            ),
        });
    }
    return Ok(ServerUrlEnum {
        name: to_ident(&format!("{seed} {placeholder}"), Case::Pascal),
        doc: Some(format!("`{placeholder}` variable of the `{label}` server URL.")),
        variants,
        default,
    });
}

/// The identifier seed for a server: an explicit `x-rust-name`, else the
/// description or URL prefixed with `server url`.
fn name_seed(server: &Server) -> Result<String> {
    if let Some(name) = extension_str(server, X_RUST_NAME)? {
        return Ok(name.to_owned());
    }
    let basis = server.description.as_deref().unwrap_or(&server.url);
    return Ok(format!("{SERVER_PREFIX} {basis}"));
}

/// A human-facing label for docs: an explicit `x-rust-name`, else the
/// description, else the URL.
fn label_of(server: &Server) -> Result<String> {
    if let Some(name) = extension_str(server, X_RUST_NAME)? {
        return Ok(name.to_owned());
    }
    return Ok(server.description.clone().unwrap_or_else(|| return server.url.clone()));
}

/// The doc comment for a server: its trimmed `description`, else a synthesised
/// sentence naming the server, so every emitted item carries documentation.
fn doc_of(server: &Server, label: &str) -> String {
    if let Some(text) = server.description.as_ref() {
        let text = text.trim();
        if !text.is_empty() {
            return text.to_owned();
        }
    }
    return format!("The `{label}` server URL.");
}

/// Ensure a seed's Rust identifier is unique across servers, appending a numeric
/// suffix on collision (`Production`, `Production 2`, ...).
fn deconflict(seed: String, used: &mut HashMap<String, usize>) -> String {
    let key = to_ident(&seed, Case::Pascal).logical().to_owned();
    let count = used.entry(key).or_insert(0);
    *count += 1;
    if *count == 1 {
        return seed;
    }
    return format!("{seed} {count}");
}

/// Extract the `{placeholder}` names from a URL, in order and de-duplicated.
///
/// A placeholder cannot contain `/`, `{`, or `}` (matching the reference
/// implementation's `\{([^/{}]+)\}`). An interrupting `/` or nested `{` discards
/// the partial name.
fn placeholders(url: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for c in url.chars() {
        match c {
            '{' => current = Some(String::new()),
            '/' => current = None,
            '}' => {
                if let Some(name) = current.take()
                    && !name.is_empty()
                    && !out.contains(&name)
                {
                    out.push(name);
                }
            }
            other => {
                if let Some(buf) = current.as_mut() {
                    buf.push(other);
                }
            }
        }
    }
    return out;
}

/// Read a string-valued extension (for example `x-rust-name`) from a server object.
fn extension_str<'a>(server: &'a Server, key: &str) -> Result<Option<&'a str>> {
    return crate::lower::extension::str_value(&server.extensions, key, &server.url);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Parse an inline document and lower its `servers:` block.
    fn lower(yaml: &str) -> Result<Option<ServerUrls>> {
        let doc: openapiv3::OpenAPI = serde_yaml::from_str(yaml).expect("parse spec");
        let spec = Spec::from_parts(doc, PathBuf::from("inline.yaml"));
        return lower_server_urls(&spec);
    }

    const PREAMBLE: &str = "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\n";

    #[test]
    fn placeholders_extracted_in_order_without_duplicates() {
        let got = placeholders("https://{host}.example.com:{port}/{base}/{host}");
        assert_eq!(got, vec!["host", "port", "base"]);
    }

    #[test]
    fn placeholder_does_not_span_a_slash() {
        assert!(placeholders("https://{a/b}.example.com").is_empty());
    }

    #[test]
    fn url_without_placeholders_has_none() {
        assert!(placeholders("https://api.example.com/v1").is_empty());
    }

    #[test]
    fn no_servers_lowers_to_none() {
        let lowered = lower(PREAMBLE).expect("lower");
        assert!(lowered.is_none());
    }

    #[test]
    fn variable_free_server_becomes_a_const() {
        let yaml = format!("{PREAMBLE}servers:\n  - url: https://api.example.com\n    description: Production\n");
        let lowered = lower(&yaml).expect("lower").expect("servers");
        assert!(lowered.enums.is_empty());
        match &lowered.servers[..] {
            [ServerUrl::Const(konst)] => {
                assert_eq!(konst.name.logical(), "SERVER_URL_PRODUCTION");
                assert_eq!(konst.url, "https://api.example.com");
            }
            other => panic!("expected a single const, got {other:?}"),
        }
    }

    #[test]
    fn enum_variable_yields_enum_param_and_default() {
        let yaml = format!(
            "{PREAMBLE}servers:\n  - url: https://api.example.com:{{port}}\n    description: Prod\n    variables:\n      \
             port:\n        default: '443'\n        enum: ['443', '8443']\n"
        );
        let lowered = lower(&yaml).expect("lower").expect("servers");
        let enom = &lowered.enums[0];
        assert_eq!(enom.name.logical(), "ServerUrlProdPort");
        assert_eq!(enom.variants.len(), 2);
        assert_eq!(
            enom.default.as_ref().map(|d| return d.logical().to_owned()),
            Some("_443".to_owned())
        );
        match &lowered.servers[0] {
            ServerUrl::Builder(builder) => match &builder.params[0].ty {
                ServerUrlParamType::Enum(name) => assert_eq!(name.logical(), "ServerUrlProdPort"),
                other => panic!("expected enum param, got {other:?}"),
            },
            other => panic!("expected a builder, got {other:?}"),
        }
    }

    #[test]
    fn unused_variable_is_skipped_and_undeclared_placeholder_is_a_str_param() {
        let yaml = format!(
            "{PREAMBLE}servers:\n  - url: https://{{tenant}}.example.com/{{basePath}}\n    description: Regional\n    \
             variables:\n      tenant:\n        default: acme\n      unused:\n        default: x\n"
        );
        let lowered = lower(&yaml).expect("lower").expect("servers");
        assert!(lowered.enums.is_empty(), "no enum variables declared");
        match &lowered.servers[0] {
            ServerUrl::Builder(builder) => {
                let names: Vec<&str> = builder.params.iter().map(|p| return p.placeholder.as_str()).collect();
                assert_eq!(
                    names,
                    vec!["tenant", "basePath"],
                    "unused var skipped, placeholders in template-appearance order"
                );
                assert!(
                    builder
                        .params
                        .iter()
                        .all(|p| return matches!(p.ty, ServerUrlParamType::Str))
                );
            }
            other => panic!("expected a builder, got {other:?}"),
        }
    }

    #[test]
    fn default_not_in_enum_is_rejected() {
        let yaml = format!(
            "{PREAMBLE}servers:\n  - url: https://api.example.com:{{port}}\n    variables:\n      port:\n        \
             default: '12345'\n        enum: ['443', '8443']\n"
        );
        let error = lower(&yaml).expect_err("default not in enum must be rejected");
        let message = error.to_string();
        assert!(
            message.contains("12345"),
            "error should name the bad default: {message}"
        );
    }

    #[test]
    fn colliding_seed_names_are_deconflicted() {
        let yaml = format!(
            "{PREAMBLE}servers:\n  - url: https://a.example.com\n    description: Prod\n  - url: https://b.example.com\n    \
             description: Prod\n"
        );
        let lowered = lower(&yaml).expect("lower").expect("servers");
        let names: Vec<&str> = lowered
            .servers
            .iter()
            .map(|server| match server {
                ServerUrl::Const(konst) => return konst.name.logical(),
                ServerUrl::Builder(builder) => return builder.name.logical(),
            })
            .collect();
        assert_eq!(names, vec!["SERVER_URL_PROD", "SERVER_URL_PROD_2"]);
    }
}
