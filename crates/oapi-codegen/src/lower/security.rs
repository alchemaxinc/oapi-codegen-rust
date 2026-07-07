//! Lowering OpenAPI security schemes and requirements into the client IR.
//!
//! Only the shared lowering pass runs here: it catalogues the document's
//! `components.securitySchemes` and resolves each operation's *effective*
//! security requirement. The server emitter ignores the result (server-side
//! auth is not generated yet); the client emitter turns supported schemes into
//! credential fields and rejects operations that require an
//! [`crate::ir::SecuritySchemeKind::Unsupported`] scheme.

use openapiv3::APIKeyLocation;
use openapiv3::ReferenceOr;
use openapiv3::SecurityRequirement;
use openapiv3::SecurityScheme as OasSecurityScheme;

use crate::ir::SecurityScheme;
use crate::ir::SecuritySchemeKind;
use crate::loader::Spec;
use crate::naming::Case;
use crate::naming::to_ident;

/// Build the ordered catalogue of security schemes declared in the document,
/// in `components.securitySchemes` declaration order.
///
/// `$ref` scheme entries are skipped (uncommon in practice); OAuth2 and OpenID
/// Connect are catalogued as [`SecuritySchemeKind::Unsupported`] so an operation
/// that requires one can be rejected with a clear message during client emit.
pub fn scheme_catalogue(spec: &Spec) -> Vec<SecurityScheme> {
    let mut schemes = Vec::new();
    for (key, entry) in spec.security_schemes() {
        let scheme = match entry {
            ReferenceOr::Item(scheme) => scheme,
            ReferenceOr::Reference { .. } => continue,
        };
        let (kind, doc) = lower_scheme(scheme);
        schemes.push(SecurityScheme {
            key: key.clone(),
            field: to_ident(key, Case::Snake),
            kind,
            doc,
        });
    }
    return schemes;
}

/// Convert a single OpenAPI security scheme into its IR kind and doc string.
fn lower_scheme(scheme: &OasSecurityScheme) -> (SecuritySchemeKind, Option<String>) {
    match scheme {
        OasSecurityScheme::HTTP {
            scheme, description, ..
        } => {
            let kind = if scheme.eq_ignore_ascii_case("bearer") {
                SecuritySchemeKind::HttpBearer
            } else if scheme.eq_ignore_ascii_case("basic") {
                SecuritySchemeKind::HttpBasic
            } else {
                SecuritySchemeKind::Unsupported(format!("HTTP authentication scheme `{scheme}` is not supported"))
            };
            return (kind, description.clone());
        }
        OasSecurityScheme::APIKey {
            location,
            name,
            description,
            ..
        } => {
            let kind = match location {
                APIKeyLocation::Header => SecuritySchemeKind::ApiKeyHeader(name.clone()),
                APIKeyLocation::Query => SecuritySchemeKind::ApiKeyQuery(name.clone()),
                APIKeyLocation::Cookie => SecuritySchemeKind::ApiKeyCookie(name.clone()),
            };
            return (kind, description.clone());
        }
        OasSecurityScheme::OAuth2 { description, .. } => {
            return (
                SecuritySchemeKind::Unsupported("OAuth2 security is not supported by the client generator".to_owned()),
                description.clone(),
            );
        }
        OasSecurityScheme::OpenIDConnect { description, .. } => {
            return (
                SecuritySchemeKind::Unsupported(
                    "OpenID Connect security is not supported by the client generator".to_owned(),
                ),
                description.clone(),
            );
        }
    }
}

/// The security requirements in effect for an operation: its own `security` if
/// present (an empty list explicitly disables auth), otherwise the document's
/// global `security`.
pub fn effective_requirements<'a>(
    operation_security: Option<&'a [SecurityRequirement]>,
    global_security: Option<&'a [SecurityRequirement]>,
) -> Option<&'a [SecurityRequirement]> {
    return operation_security.or(global_security);
}

/// The distinct scheme keys required by `requirements`, in first-seen order.
///
/// Alternatives (the outer list) and conjunctions (each map's keys) are
/// flattened to their union: the client sends every configured, required
/// credential, which is correct for the common single-scheme case and harmless
/// otherwise.
pub fn required_keys(requirements: &[SecurityRequirement]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for requirement in requirements {
        for key in requirement.keys() {
            if !keys.iter().any(|existing| return existing == key) {
                keys.push(key.clone());
            }
        }
    }
    return keys;
}
