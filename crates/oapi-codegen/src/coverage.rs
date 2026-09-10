//! OpenAPI 3.0 object keys and diagnostics before typed deserialization.

use serde_yaml::Value;

use crate::diagnostic::Warning;
use crate::diagnostic::pointer;
use crate::diagnostic::report_warnings;
use crate::error::Error;
use crate::error::Result;
use crate::lower::validate::Diagnostics;

const MAX_DEPTH: usize = 128;
const CONSTRAINT_KEYS: &[&str] = &[
    "multipleOf",
    "maximum",
    "exclusiveMaximum",
    "minimum",
    "exclusiveMinimum",
    "maxLength",
    "minLength",
    "pattern",
    "maxItems",
    "minItems",
    "uniqueItems",
    "maxProperties",
    "minProperties",
];

macro_rules! catalogue {
    ($context:expr; $($pattern:pat => $fields:expr),* $(,)?) => {
        match $context {
            $($pattern => const { $fields },)*
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    Document,
    Info,
    Contact,
    License,
    Server,
    ServerVariable,
    Paths,
    PathItem,
    Operation,
    Components,
    Schema,
    PropertySchema,
    Parameter,
    Header,
    RequestBody,
    Responses,
    Response,
    MediaType,
    Encoding,
    Example,
    Link,
    Callback,
    SecurityScheme,
    OAuthFlows,
    OAuthFlow,
    ExternalDocs,
    Tag,
    Discriminator,
    Xml,
}

#[derive(Debug, Clone, Copy)]
enum Traversal {
    Literal,
    SchemaOrBool,
    Object(Context),
    Map(Context),
    Array(Context),
}

#[derive(Debug, Clone, Copy)]
enum Handling {
    Read,
    Annotation,
    Unsupported(&'static str),
}

#[derive(Debug, Clone, Copy)]
struct Field {
    name: &'static str,
    traversal: Traversal,
    handling: Handling,
}

const fn read(name: &'static str, traversal: Traversal) -> Field {
    return Field {
        name,
        traversal,
        handling: Handling::Read,
    };
}

const fn annotation(name: &'static str, traversal: Traversal) -> Field {
    return Field {
        name,
        traversal,
        handling: Handling::Annotation,
    };
}

const fn unsupported(name: &'static str, traversal: Traversal, reason: &'static str) -> Field {
    return Field {
        name,
        traversal,
        handling: Handling::Unsupported(reason),
    };
}

fn fields(context: Context) -> &'static [Field] {
    use Context::*;
    use Traversal::*;

    return catalogue! { context;
        Document => &[
            read("openapi", Literal),
            annotation("info", Object(Info)),
            read("servers", Array(Server)),
            read("paths", Object(Paths)),
            read("components", Object(Components)),
            read("security", Literal),
            annotation("tags", Array(Tag)),
            annotation("externalDocs", Object(ExternalDocs)),
        ],
        Info => &[
            annotation("title", Literal),
            annotation("description", Literal),
            annotation("termsOfService", Literal),
            annotation("contact", Object(Contact)),
            annotation("license", Object(License)),
            annotation("version", Literal),
        ],
        Contact => &[
            annotation("name", Literal),
            annotation("url", Literal),
            annotation("email", Literal),
        ],
        License => &[annotation("name", Literal), annotation("url", Literal)],
        Server => &[
            read("url", Literal),
            read("description", Literal),
            read("variables", Map(ServerVariable)),
        ],
        ServerVariable => &[
            read("enum", Literal),
            read("default", Literal),
            read("description", Literal),
        ],
        PathItem => &[
            annotation("summary", Literal),
            annotation("description", Literal),
            read("get", Object(Operation)),
            read("put", Object(Operation)),
            read("post", Object(Operation)),
            read("delete", Object(Operation)),
            read("options", Object(Operation)),
            read("head", Object(Operation)),
            read("patch", Object(Operation)),
            read("trace", Object(Operation)),
            unsupported("servers", Array(Server), "path-level server overrides are not implemented"),
            read("parameters", Array(Parameter)),
        ],
        Operation => &[
            read("tags", Literal),
            read("summary", Literal),
            read("description", Literal),
            annotation("externalDocs", Object(ExternalDocs)),
            read("operationId", Literal),
            read("parameters", Array(Parameter)),
            read("requestBody", Object(RequestBody)),
            read("responses", Object(Responses)),
            unsupported("callbacks", Map(Callback), "callback operations are not generated"),
            unsupported("deprecated", Literal, "operation deprecation is not emitted"),
            read("security", Literal),
            unsupported("servers", Array(Server), "operation-level server overrides are not implemented"),
        ],
        Components => &[
            read("schemas", Map(Schema)),
            read("responses", Map(Response)),
            read("parameters", Map(Parameter)),
            annotation("examples", Map(Example)),
            read("requestBodies", Map(RequestBody)),
            read("headers", Map(Header)),
            read("securitySchemes", Map(SecurityScheme)),
            unsupported("links", Map(Link), "response links are not generated"),
            unsupported("callbacks", Map(Callback), "callback operations are not generated"),
        ],
        Schema | PropertySchema => &[
            annotation("title", Literal),
            read("multipleOf", Literal),
            read("maximum", Literal),
            read("exclusiveMaximum", Literal),
            read("minimum", Literal),
            read("exclusiveMinimum", Literal),
            read("maxLength", Literal),
            read("minLength", Literal),
            read("pattern", Literal),
            read("maxItems", Literal),
            read("minItems", Literal),
            read("uniqueItems", Literal),
            read("maxProperties", Literal),
            read("minProperties", Literal),
            read("required", Literal),
            read("enum", Literal),
            read("type", Literal),
            read("allOf", Array(Schema)),
            read("oneOf", Array(Schema)),
            read("anyOf", Array(Schema)),
            read("not", Object(Schema)),
            read("items", Object(Schema)),
            read("properties", Map(PropertySchema)),
            read("additionalProperties", SchemaOrBool),
            read("description", Literal),
            read("format", Literal),
            read("default", Literal),
            read("nullable", Literal),
            read("discriminator", Object(Discriminator)),
            read("readOnly", Literal),
            read("writeOnly", Literal),
            unsupported("xml", Object(Xml), "XML serialization is not implemented"),
            annotation("externalDocs", Object(ExternalDocs)),
            annotation("example", Literal),
            read("deprecated", Literal),
        ],
        Parameter => &[
            read("name", Literal),
            read("in", Literal),
            read("description", Literal),
            read("required", Literal),
            unsupported("deprecated", Literal, "parameter deprecation is not emitted"),
            unsupported("allowEmptyValue", Literal, "allowEmptyValue is not implemented"),
            read("style", Literal),
            read("explode", Literal),
            unsupported("allowReserved", Literal, "allowReserved is not implemented"),
            read("schema", Object(Schema)),
            annotation("example", Literal),
            annotation("examples", Map(Example)),
            read("content", Map(MediaType)),
        ],
        Header => &[
            read("description", Literal),
            read("required", Literal),
            unsupported("deprecated", Literal, "header deprecation is not emitted"),
            unsupported("allowEmptyValue", Literal, "header allowEmptyValue is not implemented"),
            unsupported("allowReserved", Literal, "header allowReserved is not implemented"),
            unsupported("style", Literal, "response-header serialization styles are not implemented"),
            unsupported("explode", Literal, "response-header explode is not implemented"),
            read("schema", Object(Schema)),
            annotation("example", Literal),
            annotation("examples", Map(Example)),
            read("content", Map(MediaType)),
        ],
        RequestBody => &[
            annotation("description", Literal),
            read("content", Map(MediaType)),
            read("required", Literal),
        ],
        Response => &[
            read("description", Literal),
            read("headers", Map(Header)),
            read("content", Map(MediaType)),
            unsupported("links", Map(Link), "response links are not generated"),
        ],
        MediaType => &[
            read("schema", Object(Schema)),
            annotation("example", Literal),
            annotation("examples", Map(Example)),
            unsupported("encoding", Map(Encoding), "per-property body encoding is not implemented"),
        ],
        Encoding => &[
            read("contentType", Literal),
            read("headers", Map(Header)),
            read("style", Literal),
            read("explode", Literal),
            read("allowReserved", Literal),
        ],
        Example => &[
            annotation("summary", Literal),
            annotation("description", Literal),
            annotation("value", Literal),
            annotation("externalValue", Literal),
        ],
        Link => &[
            read("operationRef", Literal),
            read("operationId", Literal),
            read("parameters", Literal),
            read("requestBody", Literal),
            annotation("description", Literal),
            read("server", Object(Server)),
        ],
        SecurityScheme => &[
            read("type", Literal),
            read("description", Literal),
            read("name", Literal),
            read("in", Literal),
            read("scheme", Literal),
            annotation("bearerFormat", Literal),
            read("flows", Object(OAuthFlows)),
            read("openIdConnectUrl", Literal),
        ],
        OAuthFlows => &[
            read("implicit", Object(OAuthFlow)),
            read("password", Object(OAuthFlow)),
            read("clientCredentials", Object(OAuthFlow)),
            read("authorizationCode", Object(OAuthFlow)),
        ],
        OAuthFlow => &[
            read("authorizationUrl", Literal),
            read("tokenUrl", Literal),
            read("refreshUrl", Literal),
            read("scopes", Literal),
        ],
        ExternalDocs => &[annotation("description", Literal), annotation("url", Literal)],
        Tag => &[
            annotation("name", Literal),
            annotation("description", Literal),
            annotation("externalDocs", Object(ExternalDocs)),
        ],
        Discriminator => &[
            unsupported("propertyName", Literal, "discriminator dispatch uses shapes rather than this property"),
            read("mapping", Literal),
        ],
        Xml => &[
            annotation("name", Literal),
            annotation("namespace", Literal),
            annotation("prefix", Literal),
            annotation("attribute", Literal),
            annotation("wrapped", Literal),
        ],
        Paths | Responses | Callback => &[],
    };
}

impl Context {
    fn references(self) -> bool {
        return matches!(
            self,
            Self::Schema
                | Self::PropertySchema
                | Self::PathItem
                | Self::Parameter
                | Self::Header
                | Self::RequestBody
                | Self::Response
                | Self::Example
                | Self::Link
                | Self::SecurityScheme
                | Self::Callback
        );
    }
}

struct Sweep<'a> {
    document: &'a str,
    warnings: Vec<Warning>,
    problems: Diagnostics,
}

pub(crate) fn check(document: &str, value: &Value) -> Result<()> {
    let sweep = inspect(document, value);
    report_warnings(document, &sweep.warnings);
    return sweep.problems.into_result();
}

fn inspect<'a>(document: &'a str, value: &Value) -> Sweep<'a> {
    let mut sweep = Sweep {
        document,
        warnings: Vec::new(),
        problems: Diagnostics::new(),
    };
    sweep.object(value, Context::Document, "", 0);
    return sweep;
}

impl Sweep<'_> {
    fn invalid(&mut self, path: &str, reason: impl Into<String>) {
        self.problems.push(Error::InvalidSpec {
            document: self.document.to_owned(),
            path: path.to_owned(),
            reason: reason.into(),
        });
    }

    fn warn(&mut self, path: &str, message: impl Into<String>) {
        self.warnings.push(Warning::new(path, message));
    }

    fn object(&mut self, value: &Value, context: Context, path: &str, depth: usize) {
        if depth > MAX_DEPTH {
            self.invalid(path, format!("OpenAPI object nesting exceeds {MAX_DEPTH} levels"));
            return;
        }
        let Some(mapping) = value.as_mapping() else {
            self.invalid(path, format!("{context:?} must be an object"));
            return;
        };
        let reference = context.references() && mapping.contains_key("$ref");
        for (key, child) in mapping {
            let key = match key {
                Value::String(key) => key.clone(),
                Value::Number(number) if context == Context::Responses => number.to_string(),
                _ => {
                    self.invalid(path, "OpenAPI object keys must be strings");
                    continue;
                }
            };
            let at = pointer(path, &key);
            if key == "$ref" && context.references() {
                if child.as_str().is_none() {
                    self.invalid(&at, "$ref must be a string");
                }
                continue;
            }
            if reference && context != Context::PathItem {
                self.warn(&at, "siblings of $ref are ignored in OpenAPI 3.0");
            }
            if key.starts_with("x-") {
                self.extension(&key, context, &at);
                continue;
            }
            let dynamic = match context {
                Context::Paths if key.starts_with('/') => Some(Context::PathItem),
                Context::Responses if response_key(&key) => Some(Context::Response),
                Context::Callback => Some(Context::PathItem),
                _ => None,
            };
            if let Some(next) = dynamic {
                self.object(child, next, &at, depth + 1);
                continue;
            }
            let Some(field) = fields(context).iter().find(|field| return field.name == key) else {
                self.invalid(&at, format!("unknown OpenAPI 3.0 key `{key}` in {context:?}"));
                continue;
            };
            if !reference {
                if let Handling::Unsupported(reason) = field.handling
                    && child.as_bool() != Some(false)
                {
                    self.warn(&at, reason);
                }
                self.value_notes(context, &key, child, &at);
            }
            self.walk(child, field.traversal, &at, depth + 1);
        }
        if !reference {
            self.object_notes(value, context, path);
        }
    }

    fn walk(&mut self, value: &Value, traversal: Traversal, path: &str, depth: usize) {
        match traversal {
            Traversal::Literal => {}
            Traversal::SchemaOrBool if value.is_bool() => {}
            Traversal::SchemaOrBool => self.object(value, Context::Schema, path, depth),
            Traversal::Object(context) => self.object(value, context, path, depth),
            Traversal::Map(context) => {
                if let Some(mapping) = value.as_mapping() {
                    for (name, child) in mapping {
                        let Some(name) = name.as_str() else {
                            self.invalid(path, "OpenAPI map names must be strings");
                            continue;
                        };
                        self.object(child, context, &pointer(path, name), depth);
                    }
                } else {
                    self.invalid(path, "this OpenAPI field must be a map");
                }
            }
            Traversal::Array(context) => {
                if let Some(sequence) = value.as_sequence() {
                    for (index, child) in sequence.iter().enumerate() {
                        self.object(child, context, &pointer(path, &index.to_string()), depth);
                    }
                } else {
                    self.invalid(path, "this OpenAPI field must be an array");
                }
            }
        }
    }

    fn extension(&mut self, key: &str, context: Context, path: &str) {
        if key.starts_with("x-go-") {
            return;
        }
        let handled = match context {
            Context::PropertySchema => matches!(
                key,
                "x-rust-type"
                    | "x-rust-name"
                    | "x-rust-derive"
                    | "x-rust-serde-skip"
                    | "x-omitempty"
                    | "x-order"
                    | "x-deprecated-reason"
                    | "x-enum-varnames"
                    | "x-enumNames"
            ),
            Context::Schema => matches!(
                key,
                "x-rust-type"
                    | "x-rust-name"
                    | "x-rust-derive"
                    | "x-deprecated-reason"
                    | "x-enum-varnames"
                    | "x-enumNames"
            ),
            Context::Operation => key == "x-rust-name",
            _ => false,
        };
        if !handled {
            self.warn(path, "this extension is not implemented here and is ignored");
        }
    }

    fn value_notes(&mut self, context: Context, key: &str, value: &Value, path: &str) {
        if matches!(context, Context::Document | Context::Operation)
            && key == "security"
            && let Some(requirements) = value.as_sequence()
        {
            if requirements.len() > 1 {
                self.warn(path, "security alternatives are flattened into a list of scheme names");
            }
            if requirements.iter().any(|requirement| {
                return requirement.as_mapping().is_some_and(|schemes| {
                    return schemes.values().any(|scopes| {
                        return scopes.as_sequence().is_some_and(|scopes| return !scopes.is_empty());
                    });
                });
            }) {
                self.warn(path, "security scopes are not enforced by the generated code");
            }
        }
        if matches!(context, Context::Schema | Context::PropertySchema) {
            match key {
                "oneOf" | "anyOf" => self.warn(
                    path,
                    "Rust deserialization checks do not enforce all schema constraints, which can affect union match counts",
                ),
                "allOf"
                    if value.as_sequence().is_some_and(|members| {
                        return match members.as_slice() {
                            [] => false,
                            [member] => member.get("$ref").is_none(),
                            _ => true,
                        };
                    }) =>
                {
                    self.warn(
                        path,
                        "allOf merges properties rather than validating every member independently",
                    );
                }
                "default" if value.is_null() => {
                    self.warn(
                        path,
                        "the parser discards a null default, so an absent property does not receive explicit null",
                    );
                }
                "default" if context == Context::Schema => {
                    self.warn(
                        path,
                        "defaults are applied only at supported property and query-parameter uses",
                    );
                }
                _ => {}
            }
        }
    }

    fn object_notes(&mut self, value: &Value, context: Context, path: &str) {
        match context {
            Context::RequestBody if value.get("required").and_then(Value::as_bool) != Some(true) => {
                self.warn(
                    path,
                    "optional request bodies are generated as required when a body type is emitted",
                );
            }
            Context::MediaType if value.get("schema").is_none() => {
                self.warn(path, "a media entry without a schema does not generate a body type");
            }
            Context::Schema | Context::PropertySchema => self.schema_notes(value, context, path),
            _ => {}
        }
    }

    fn schema_notes(&mut self, value: &Value, context: Context, path: &str) {
        if value.get("x-rust-type").is_some() && value.get("allOf").is_some() {
            self.warn(
                path,
                "constraints inherited through allOf are not enforced for x-rust-type",
            );
        }
        if value.get("nullable").and_then(Value::as_bool) == Some(true)
            && let Ok(schema) = serde_yaml::from_value::<openapiv3::Schema>(value.clone())
        {
            let location = path.split("/schema/").next().unwrap_or(path);
            if !location.starts_with("/components/schemas/")
                && (location.contains("/parameters/")
                    || location.contains("/headers/")
                    || location.contains("/content/multipart~1form-data/")
                    || location.contains("/content/application~1x-www-form-urlencoded/")
                    || location.contains("/content/text~1plain/"))
            {
                self.warn(
                    path,
                    "nullable values have no supported null representation in this wire format",
                );
            }
            if crate::lower::default::unsupported_nullable_default(&schema) {
                self.warn(
                    &pointer(path, "default"),
                    "this nullable default has no supported Rust literal and is ignored",
                );
            }
            if crate::lower::constraints::unsupported_nullable_constraints(&schema)
                && CONSTRAINT_KEYS.iter().any(|key| return value.get(*key).is_some())
            {
                self.warn(path, "constraints on this nullable value type are not enforced");
            }
        }
        let kind = value.get("type").and_then(Value::as_str);
        if let Some(kind) = kind
            && !matches!(kind, "string" | "integer" | "number" | "boolean" | "object" | "array")
        {
            self.invalid(
                &pointer(path, "type"),
                format!("`{kind}` is not an OpenAPI 3.0 schema type"),
            );
        }
        if context == Context::Schema && CONSTRAINT_KEYS.iter().any(|key| return value.get(*key).is_some()) {
            self.warn(
                path,
                "constraints are enforced only at supported field uses, not on type aliases or array items",
            );
        }
        if value.get("x-rust-derive").is_some() && value.get("x-rust-type").is_none() {
            self.warn(
                &pointer(path, "x-rust-derive"),
                "x-rust-derive requires x-rust-type and is otherwise ignored",
            );
        }
        if matches!(kind, Some("number" | "boolean")) && value.get("enum").is_some() {
            self.warn(
                &pointer(path, "enum"),
                "number and boolean enum restrictions are not enforced",
            );
        }
        if let Some(format) = value.get("format").and_then(Value::as_str) {
            let handled = match kind {
                Some("string") => matches!(format, "date" | "date-time" | "byte" | "binary" | "password" | "uuid"),
                Some("integer") => matches!(format, "int32" | "int64"),
                Some("number") => matches!(format, "float" | "double"),
                _ => false,
            };
            if !handled {
                self.warn(
                    &pointer(path, "format"),
                    "this format is not implemented and the base type is used",
                );
            }
        }
        if kind == Some("object")
            && value.get("additionalProperties").and_then(Value::as_bool) == Some(false)
            && value
                .get("properties")
                .and_then(Value::as_mapping)
                .is_none_or(serde_yaml::Mapping::is_empty)
        {
            self.warn(
                &pointer(path, "additionalProperties"),
                "an object without declared properties becomes a map and does not reject additional properties",
            );
        }
    }
}

fn response_key(key: &str) -> bool {
    if key == "default" {
        return true;
    }
    let mut bytes = key.bytes();
    return matches!(bytes.next(), Some(b'1'..=b'5'))
        && matches!(
            (bytes.next(), bytes.next(), bytes.next()),
            (Some(b'0'..=b'9'), Some(b'0'..=b'9'), None) | (Some(b'X'), Some(b'X'), None)
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inspect_yaml(yaml: &str) -> Sweep<'static> {
        let value = serde_yaml::from_str(yaml).expect("valid YAML");
        return inspect("spec.yaml", &value);
    }

    #[test]
    fn unknown_keys_are_rejected_in_nested_objects() {
        for yaml in [
            "inf: {}",
            "info: {titel: Demo}",
            "components: {schemas: {Widget: {type: string, const: x}}}",
            "paths: {/widgets: {get: {operationID: list}}}",
            "components: {schemas: {Widget: {properties: {name: {typo: string}}}}}",
            "components: {schemas: {Widget: {xml: {nam: widget}}}}",
            "components: {examples: {payload: {externalvalue: url}}}",
            "components: {securitySchemes: {auth: {flows: {password: {tokenURL: url}}}}}",
            "paths: {widgets: {}}",
            "paths: {/widgets: {get: {responses: {20X: {description: wrong}}}}}",
        ] {
            let sweep = inspect_yaml(yaml);
            assert!(!sweep.problems.is_empty(), "accepted {yaml}");
        }
    }

    #[test]
    fn invalid_structures_and_schema_types_are_rejected() {
        for yaml in [
            "info: []",
            "components: {schemas: []}",
            "servers: {}",
            "components: {schemas: {Widget: {type: stirng}}}",
            "components: {schemas: {Widget: {type: 'null'}}}",
            "components: {schemas: {Widget: {$ref: 42, type: string}}}",
            "components: {schemas: {Widget: {xml: false}}}",
            "components: {schemas: {Widget: {items: true}}}",
            "components: {schemas: {Widget: {additionalProperties: []}}}",
        ] {
            assert!(!inspect_yaml(yaml).problems.is_empty(), "accepted {yaml}");
        }
        for value in ["true", "false", "{type: string}"] {
            let yaml = format!("components: {{schemas: {{Widget: {{type: object, additionalProperties: {value}}}}}}}");
            assert!(inspect_yaml(&yaml).problems.is_empty(), "{yaml}");
        }
    }

    #[test]
    fn supported_shapes_and_documented_annotations_are_quiet() {
        let sweep = inspect_yaml(
            "
openapi: 3.0.3
info:
  title: Demo
  version: 1.0.0
  contact: {name: Support, url: 'https://example.com', email: support@example.com}
  license: {name: MIT, url: 'https://example.com/license'}
tags: [{name: widgets, externalDocs: {url: 'https://example.com/widgets'}}]
paths: {}
components:
  schemas:
    Widget:
      type: object
      title: A widget
      description: The model.
      required: [id]
      properties:
        id: {type: string, example: {arbitrary: true}}
",
        );
        assert!(sweep.problems.is_empty());
        assert!(sweep.warnings.is_empty(), "{:?}", sweep.warnings);
    }

    #[test]
    fn header_flags_are_recognized_and_default_values_are_quiet() {
        for flag in [false, true] {
            let yaml = format!(
                "components: {{headers: {{X-Test: {{schema: {{type: string}}, allowEmptyValue: {flag}, allowReserved: {flag}}}}}}}"
            );
            let sweep = inspect_yaml(&yaml);
            assert!(sweep.problems.is_empty());
            assert_eq!(sweep.warnings.is_empty(), !flag);
        }
    }

    #[test]
    fn deeply_nested_objects_stop_at_the_inspection_limit() {
        let mut value = Value::Mapping(serde_yaml::Mapping::new());
        for _ in 0..MAX_DEPTH + 1 {
            let mut mapping = serde_yaml::Mapping::new();
            mapping.insert(Value::String("items".to_owned()), value);
            value = Value::Mapping(mapping);
        }
        let mut sweep = inspect_yaml("{}");
        sweep.object(&value, Context::Schema, "/schema", 0);
        let error = sweep.problems.into_result().expect_err("nesting limit");
        assert!(error.to_string().contains("nesting exceeds"));
    }

    #[test]
    fn literal_maps_and_user_names_are_not_spec_objects() {
        let sweep = inspect_yaml(
            "
components:
  schemas:
    x-model:
      type: object
      properties:
        const: {type: string}
      example: {const: arbitrary, typo: {anything: true}}
      default: {unevaluatedProperties: false}
      enum: [{other: {anything: true}}]
      discriminator: {propertyName: kind, mapping: {arbitrary: '#/components/schemas/x-model'}}
  examples:
    x-example: {value: {notAnOpenAPIKey: true}}
  links:
    next: {parameters: {arbitrary: '$response.body#/id'}, requestBody: {anything: true}}
  securitySchemes:
    auth:
      type: oauth2
      flows:
        password: {tokenUrl: /token, scopes: {arbitrary: description}}
security: [{arbitrary: [custom]}]
",
        );
        assert!(sweep.problems.is_empty());
        assert!(
            !sweep
                .warnings
                .iter()
                .any(|warning| return warning.path.contains("typo"))
        );
    }

    #[test]
    fn unsupported_features_warn_and_their_children_are_still_checked() {
        let sweep = inspect_yaml(
            "paths: {/widgets: {get: {callbacks: {event: {'{$request.body#/url}': {post: {responses: {default: {description: ok}}}}}}}}}",
        );
        assert!(sweep.problems.is_empty());
        assert!(
            sweep
                .warnings
                .iter()
                .any(|warning| return warning.path.ends_with("/callbacks"))
        );
        let invalid = inspect_yaml(
            "paths: {/widgets: {get: {callbacks: {event: {'{$request.body#/url}': {post: {typo: true}}}}}}}",
        );
        assert!(!invalid.problems.is_empty());
    }

    #[test]
    fn references_report_ignored_siblings() {
        let sweep =
            inspect_yaml("components: {schemas: {Widget: {$ref: '#/components/schemas/Other', description: ignored}}}");
        assert!(sweep.problems.is_empty());
        assert!(
            sweep
                .warnings
                .iter()
                .any(|warning| return warning.message.contains("siblings"))
        );
        let invalid =
            inspect_yaml("components: {schemas: {Widget: {$ref: '#/components/schemas/Other', requird: [id]}}}");
        assert!(!invalid.problems.is_empty());
    }

    #[test]
    fn extensions_are_opaque_but_unhandled_extensions_warn() {
        let sweep = inspect_yaml(
            "x-vendor: {arbitrary: true}\nx-go-custom: true\ncomponents: {schemas: {Widget: {type: string, x-rust-type: 'String'}}}",
        );
        assert!(sweep.problems.is_empty());
        assert_eq!(sweep.warnings.len(), 1);
        assert_eq!(sweep.warnings.first().expect("warning").path, "/x-vendor");
    }

    #[test]
    fn pointers_escape_property_and_path_names() {
        let sweep = inspect_yaml("paths: {'/a~b': {get: {typo: true}}}");
        let error = sweep.problems.into_result().expect_err("unknown key");
        assert!(matches!(error, Error::InvalidSpec { path, .. } if path == "/paths/~1a~0b/get/typo"));
    }

    #[test]
    fn response_keys_allow_exact_codes_ranges_and_default() {
        for key in ["100", "200", "599", "2XX", "default"] {
            assert!(response_key(key), "{key}");
        }
        for key in ["20", "2000", "600", "2xx", "20X", "foo"] {
            assert!(!response_key(key), "{key}");
        }
        let sweep = inspect_yaml("paths: {/widgets: {get: {responses: {200: {description: ok}}}}}");
        assert!(sweep.problems.is_empty());
    }

    #[test]
    fn schema_limitations_are_reported_without_changing_generation() {
        for (schema, keyword) in [
            ("{type: number, enum: [1.5]}", "enum"),
            ("{type: string, format: custom}", "format"),
            ("{type: string, nullable: true, default: null}", "default"),
            ("{oneOf: [{type: string}, {type: integer}]}", "oneOf"),
            ("{anyOf: [{type: string}, {type: integer}]}", "anyOf"),
            ("{type: object, additionalProperties: false}", "additionalProperties"),
            (
                "{allOf: [{type: object, properties: {name: {type: string}}, additionalProperties: false}]}",
                "allOf",
            ),
        ] {
            let yaml = format!("components: {{schemas: {{Widget: {schema}}}}}");
            let sweep = inspect_yaml(&yaml);
            assert!(sweep.problems.is_empty(), "{schema}");
            assert!(
                sweep
                    .warnings
                    .iter()
                    .any(|warning| return warning.path.ends_with(keyword)),
                "{schema}",
            );
            if matches!(keyword, "oneOf" | "anyOf") {
                assert!(sweep.warnings.iter().any(|warning| {
                    return warning.message.contains("Rust deserialization checks")
                        && warning.message.contains("can affect union match counts");
                }));
            }
        }
    }

    #[test]
    fn nullable_limitations_warn_before_the_typed_parse() {
        for (schema, message) in [
            (
                "{type: string, nullable: true, default: null}",
                "parser discards a null default",
            ),
            (
                "{type: array, nullable: true, items: {type: string}, default: [value]}",
                "no supported Rust literal",
            ),
            (
                "{type: string, nullable: true, x-rust-type: String, minLength: 2}",
                "constraints on this nullable value type",
            ),
        ] {
            let yaml =
                format!("components: {{schemas: {{Container: {{type: object, properties: {{field: {schema}}}}}}}}}");
            let sweep = inspect_yaml(&yaml);
            assert!(sweep.problems.is_empty());
            assert!(
                sweep
                    .warnings
                    .iter()
                    .any(|warning| return warning.message.contains(message))
            );
        }

        let supported = inspect_yaml("components: {schemas: {Text: {type: string, nullable: true}}}");
        assert!(supported.warnings.is_empty());
    }

    #[test]
    fn nullable_wire_parameters_and_custom_allof_constraints_warn() {
        let report = inspect_yaml(
            "paths:
  /probe:
    get:
      parameters:
        - name: query
          in: query
          schema: {type: string, nullable: true}
      responses: {}
components:
  schemas:
    Custom:
      nullable: true
      x-rust-type: i64
      allOf:
        - {type: string, minLength: 2}",
        );
        assert!(report.warnings.iter().any(|warning| {
            return warning.message.contains("no supported null representation");
        }));
        assert!(report.warnings.iter().any(|warning| {
            return warning.message.contains("inherited through allOf");
        }));
        let json = inspect_yaml(
            "paths:
  /probe:
    post:
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                headers:
                  type: array
                  items: {type: string, nullable: true}
      responses: {}",
        );
        assert!(!json.warnings.iter().any(|warning| {
            return warning.message.contains("no supported null representation");
        }));
    }
}
