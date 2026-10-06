//! OpenAPI 3.0 object keys and diagnostics before typed deserialization.

use serde_yaml::Value;

use crate::diagnostic::Warning;
use crate::diagnostic::pointer;
use crate::diagnostic::report_warnings;
use crate::error::Error;
use crate::error::Result;
use crate::lower::validate::Diagnostics;

const MAX_DEPTH: usize = 128;
/// The schemas that the lowering reads by type, found from what the run
/// lowers: the parameters, request bodies, and responses of each operation the
/// run keeps, and, in a referenced document, every component parameter,
/// request body, and response, because another document's operations name
/// them.
///
/// A path item's own parameters count for an operation unless it declares a
/// parameter of the same name and location, as the lowering overrides them. A
/// `$ref` to a component parameter, request body, or response is followed the
/// way the resolver follows it. A component schema that is itself a `$ref` is
/// an alias, and the lowering resolves an alias chain, so every name on the
/// chain is collected. The lowering reads such a component by type even when a
/// model names it too, so the model use does not lift the mark.
///
/// A run that lowers no operation reads nothing by type, in the referenced
/// documents as well: a models-only run turns a custom type schema into an
/// alias and nothing else.
fn read_by_type(document: &Value, run: &Run) -> ByType {
    let mut found = ByType::default();
    if !run.operations {
        return found;
    }
    if run.referenced {
        // The run lowers the root document's operations only, so this document
        // is read where the root's operations reach into it.
        for path in &run.referenced_uses {
            let Some((kind, name)) = component_path_parts(path) else {
                continue;
            };
            let name = name.replace("~1", "/").replace("~0", "~");
            let value = document
                .get("components")
                .and_then(|components| return components.get(kind))
                .and_then(|values| return values.get(name.as_str()));
            if let (Some(value), Some(read)) = (value, Use::for_kind(kind)) {
                read.collect(document, path, value, &mut found);
            }
        }
        return found;
    }
    let paths = document.get("paths").and_then(Value::as_mapping);
    for (route, item) in paths.into_iter().flatten() {
        let Some(route) = route.as_str() else {
            continue;
        };
        let item_path = pointer("/paths", route);
        let kept: Vec<(&str, &Value)> = OPERATION_KEYS
            .iter()
            .filter_map(|method| return item.get(*method).map(|operation| return (*method, operation)))
            .filter(|(_, operation)| return !run_removes(run, operation))
            .collect();
        for (method, operation) in kept {
            let operation_path = pointer(&item_path, method);
            for (path, parameter) in effective_parameters(document, &item_path, item, &operation_path, operation) {
                Use::Parameter.collect(document, &path, parameter, &mut found);
            }
            if let Some(body) = operation.get("requestBody") {
                Use::Body.collect(document, &pointer(&operation_path, "requestBody"), body, &mut found);
            }
            let responses = operation.get("responses").and_then(Value::as_mapping);
            for (code, response) in responses.into_iter().flatten() {
                let code = match code {
                    Value::String(code) => code.clone(),
                    Value::Number(code) => code.to_string(),
                    _ => continue,
                };
                let path = pointer(&pointer(&operation_path, "responses"), &code);
                Use::Response.collect(document, &path, response, &mut found);
            }
        }
    }
    return found;
}

/// What the lowering reads by type: the schemas of this document, and the
/// components of other documents that the kept operations reach through a
/// cross-file `$ref`, each as the file and the component's path in it.
#[derive(Debug, Default)]
pub(crate) struct ByType {
    schemas: Vec<ReadByType>,
    pub(crate) external: Vec<(String, String)>,
}

/// The kind and name in a component path such as `/components/parameters/N`.
fn component_path_parts(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("/components/")?;
    let (kind, name) = rest.split_once('/')?;
    return (!name.contains('/')).then_some((kind, name));
}

/// Whether the filters of `run` remove `operation`.
fn run_removes(run: &Run, operation: &Value) -> bool {
    let tags: Vec<String> = operation
        .get("tags")
        .and_then(Value::as_sequence)
        .map(|tags| return tags.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let id = operation.get("operationId").and_then(Value::as_str);
    return crate::filter::removes_operation(&run.filters, &tags, id);
}

/// The parameters an operation reads, with the path of each: its own, and the
/// path item's own except where the operation declares the same name and
/// location.
fn effective_parameters<'a>(
    document: &'a Value,
    item_path: &str,
    item: &'a Value,
    operation_path: &str,
    operation: &'a Value,
) -> Vec<(String, &'a Value)> {
    let listed = |owner: &'a Value, owner_path: &str| {
        let parameters = owner.get("parameters").and_then(Value::as_sequence);
        let list = pointer(owner_path, "parameters");
        return parameters
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(index, parameter)| return (pointer(&list, &index.to_string()), parameter))
            .collect::<Vec<_>>();
    };
    let identity = |parameter: &Value| {
        let Followed::Here(_, parameter) = follow_references(document, "parameters", "", parameter) else {
            return None;
        };
        let location = parameter.get("in")?.as_str()?.to_owned();
        // A header name has no case, and the lowering overrides it without one.
        let name = parameter.get("name")?.as_str()?;
        let name = if location == "header" {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        };
        return Some((name, location));
    };
    let own = listed(operation, operation_path);
    let declared: std::collections::HashSet<_> = own
        .iter()
        .filter_map(|(_, parameter)| return identity(parameter))
        .collect();
    let inherited = listed(item, item_path)
        .into_iter()
        .filter(|(_, parameter)| return identity(parameter).is_none_or(|key| return !declared.contains(&key)));
    return inherited.chain(own).collect();
}

/// One kind of use the lowering reads by type.
#[derive(Debug, Clone, Copy)]
enum Use {
    /// A parameter: its schema is read through its collections. The parameter
    /// lowering resolves a `$ref` and maps the type, so a named component is
    /// read the same way.
    Parameter,
    /// A request body: an inline schema is read through its collections, and a
    /// multipart one through its properties. A named component is read by
    /// type only for a multipart body; the other bodies reuse the model, which
    /// reads `x-rust-type`.
    Body,
    /// A response: an inline schema is read through its collections. A named
    /// component reuses the model.
    Response,
}

impl Use {
    fn kind(self) -> &'static str {
        return match self {
            Use::Parameter => "parameters",
            Use::Body => "requestBodies",
            Use::Response => "responses",
        };
    }

    fn for_kind(kind: &str) -> Option<Self> {
        return [Use::Parameter, Use::Body, Use::Response]
            .into_iter()
            .find(|read| return read.kind() == kind);
    }

    fn collect(self, document: &Value, path: &str, value: &Value, found: &mut ByType) {
        let (path, value) = match follow_references(document, self.kind(), path, value) {
            Followed::Here(path, value) => (path, value),
            Followed::Elsewhere(file, path) => {
                found.external.push((file, path));
                return;
            }
            Followed::Nowhere => return,
        };
        let found = &mut found.schemas;
        if let Use::Parameter = self {
            // The header lowering skips the headers the framework owns.
            let reserved = value.get("in").and_then(Value::as_str) == Some("header")
                && value.get("name").and_then(Value::as_str).is_some_and(|name| {
                    return crate::lower::paths::IGNORED_HEADER_NAMES
                        .iter()
                        .any(|ignored| return ignored.eq_ignore_ascii_case(name));
                });
            if reserved {
                return;
            }
            schema_marks(document, &pointer(&path, "schema"), value.get("schema"), found);
            return;
        }
        // The body lowering reads the first entry of each kind it supports, in
        // its own order, and ignores the rest.
        let priority: &[crate::ir::BodyKind] = match self {
            Use::Body => &crate::lower::paths::REQUEST_BODY_PRIORITY,
            _ => &crate::lower::paths::RESPONSE_BODY_PRIORITY,
        };
        let content = value.get("content").and_then(Value::as_mapping);
        for wanted in priority {
            let selected = content.into_iter().flatten().find(|(name, _)| {
                return name
                    .as_str()
                    .is_some_and(|name| return crate::lower::paths::media_type_kind(name) == Some(*wanted));
            });
            let Some((name, media)) = selected else {
                continue;
            };
            let Some(name) = name.as_str() else {
                continue;
            };
            let schema_path = pointer(&pointer(&pointer(&path, "content"), name), "schema");
            let schema = media.get("schema");
            if *wanted == crate::ir::BodyKind::Multipart {
                multipart_marks(document, &schema_path, schema, found);
            } else if schema_reference(Some(media)).is_none() {
                // An inline body or response is read by type.
                found.push(ReadByType::new(schema_path));
            } else {
                // A named body or response reuses the model, which reads the
                // extension.
            }
        }
    }
}

/// Mark `schema` at `path` as read by type, following an alias chain when it
/// is a `$ref`.
fn schema_marks(document: &Value, path: &str, schema: Option<&Value>, found: &mut Vec<ReadByType>) {
    match schema
        .and_then(|schema| return schema.get("$ref"))
        .and_then(Value::as_str)
    {
        Some(reference) => found.extend(alias_chain(document, reference).map(ReadByType::new)),
        None => found.push(ReadByType::new(path.to_owned())),
    }
}

/// Mark a multipart body schema the way the multipart lowering reads it: each
/// property the request sends, by type. The object itself is not marked: a
/// `readOnly` property travels in no request, so the lowering skips it before
/// it reads the type, and when the object carries `x-rust-type` that property
/// stays below the replacement while the sent ones are read through it.
fn multipart_marks(document: &Value, path: &str, schema: Option<&Value>, found: &mut Vec<ReadByType>) {
    let Some(schema) = schema else {
        return;
    };
    // The chain of aliases ends at the object the fields are read from.
    let (object_path, object) = match schema.get("$ref").and_then(Value::as_str) {
        Some(reference) => {
            let Some(last) = alias_chain(document, reference).last() else {
                return;
            };
            let name = last
                .rsplit('/')
                .next()
                .unwrap_or("")
                .replace("~1", "/")
                .replace("~0", "~");
            let Some(object) = document
                .get("components")
                .and_then(|components| return components.get("schemas"))
                .and_then(|schemas| return schemas.get(name.as_str()))
            else {
                return;
            };
            (last, object)
        }
        None => (path.to_owned(), schema),
    };
    let properties = object.get("properties").and_then(Value::as_mapping);
    for (name, property) in properties.into_iter().flatten() {
        let Some(name) = name.as_str() else {
            continue;
        };
        if is_read_only(document, property) {
            continue;
        }
        schema_marks(
            document,
            &pointer(&pointer(&object_path, "properties"), name),
            Some(property),
            found,
        );
    }
}

/// Whether a multipart field is `readOnly`, on the property itself, on the
/// one-member `allOf` wrapper around its `$ref`, or on the schema that its
/// `$ref` names through any alias chain. The multipart lowering resolves the
/// reference and skips such a field before it reads the type.
fn is_read_only(document: &Value, property: &Value) -> bool {
    let marked = |schema: &Value| return schema.get("readOnly").and_then(Value::as_bool) == Some(true);
    if marked(property) {
        return true;
    }
    let reference = property.get("$ref").and_then(Value::as_str).or_else(|| {
        let members = property.get("allOf")?.as_sequence()?;
        let [member] = members.as_slice() else {
            return None;
        };
        return member.get("$ref")?.as_str();
    });
    let Some(reference) = reference else {
        return false;
    };
    return alias_chain(document, reference).any(|path| {
        let Some((_, name)) = component_path_parts(&path) else {
            return false;
        };
        let name = name.replace("~1", "/").replace("~0", "~");
        return document
            .get("components")
            .and_then(|components| return components.get("schemas"))
            .and_then(|schemas| return schemas.get(name.as_str()))
            .is_some_and(marked);
    });
}

/// Where a chain of component references leads.
enum Followed<'a> {
    /// A value of this document, with its path.
    Here(String, &'a Value),
    /// A component of another document: the file, and the component's path in
    /// it. The resolver reads that document, so the inspection of that
    /// document reads the component as used.
    Elsewhere(String, String),
    /// Nothing the resolver would reach.
    Nowhere,
}

/// `value` with its `path`, or the component of `kind` it names when it is a
/// `$ref`, following a chain of such references the way the resolver does, up
/// to its depth. The last lookup may reach the value itself, as the resolver's
/// last step does; one more reference is one too many.
fn follow_references<'a>(document: &'a Value, kind: &str, path: &str, value: &'a Value) -> Followed<'a> {
    let mut current = (path.to_owned(), value);
    for _ in 0..crate::loader::MAX_REF_DEPTH {
        let Some(reference) = current.1.get("$ref").and_then(Value::as_str) else {
            return Followed::Here(current.0, current.1);
        };
        let Some(name) = crate::loader::ref_component_name(reference, kind) else {
            return Followed::Nowhere;
        };
        let component_path = pointer(&pointer("/components", kind), name);
        if let Some(file) = crate::loader::ref_file_part(reference) {
            return Followed::Elsewhere(file.to_owned(), component_path);
        }
        let component = document
            .get("components")
            .and_then(|components| return components.get(kind))
            .and_then(|values| return values.get(name));
        let Some(component) = component else {
            return Followed::Nowhere;
        };
        current = (component_path, component);
    }
    return match current.1.get("$ref") {
        None => Followed::Here(current.0, current.1),
        Some(_) => Followed::Nowhere,
    };
}

/// The `$ref` in the `schema` of `holder`, when it has one.
fn schema_reference(holder: Option<&Value>) -> Option<&str> {
    return holder?.get("schema")?.get("$ref")?.as_str();
}

/// The paths of every component schema on the alias chain that starts at
/// `reference`, in the order the resolver follows them and up to the depth it
/// follows. A component schema that is itself a `$ref` is an alias.
fn alias_chain<'a>(document: &'a Value, reference: &str) -> impl Iterator<Item = String> + 'a {
    let mut current = reference.strip_prefix("#/components/schemas/").map(str::to_owned);
    let mut budget = crate::loader::MAX_REF_DEPTH;
    return std::iter::from_fn(move || {
        let name = current.take()?;
        budget = budget.checked_sub(1)?;
        current = document
            .get("components")
            .and_then(|components| return components.get("schemas"))
            .and_then(|schemas| return schemas.get(&name))
            .and_then(|schema| return schema.get("$ref"))
            .and_then(Value::as_str)
            .and_then(|next| return next.strip_prefix("#/components/schemas/"))
            .map(str::to_owned);
        return Some(pointer("/components/schemas", &name));
    });
}

/// A schema the lowering reads by type. The read enters `items`,
/// `additionalProperties`, and a one-member `allOf`, and stops at an object
/// with properties, which is lowered as a model.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadByType {
    /// The path of the schema.
    root: String,
}

impl ReadByType {
    fn new(root: String) -> Self {
        return Self { root };
    }

    /// Whether `path` is this schema or lies below it along the read.
    fn covers(&self, path: &str) -> bool {
        let Some(rest) = path.strip_prefix(self.root.as_str()) else {
            return false;
        };
        if !rest.is_empty() && !rest.starts_with('/') {
            return false;
        }
        let mut segments = rest.split('/').skip(1).peekable();
        while let Some(segment) = segments.next() {
            match segment {
                "items" | "additionalProperties" => {}
                "allOf" if segments.next_if_eq(&"0").is_some() => {}
                _ => return false,
            }
        }
        return true;
    }
}

/// The keys of a path item that hold an operation.
const OPERATION_KEYS: &[&str] = &["get", "put", "post", "delete", "options", "head", "patch", "trace"];

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

/// What a run generates and keeps, as far as the inspection needs to know.
#[derive(Debug, Default, Clone)]
pub(crate) struct Run {
    /// Whether the run generates the server. Only the server reads a query
    /// parameter, so only it checks the constraints of one.
    pub(crate) server: bool,
    /// Whether the run lowers operations at all, for a server or a client. A
    /// models-only run reads no parameter and no body.
    pub(crate) operations: bool,
    /// Whether the document is one that another document references. The run
    /// lowers the root document's operations only, so such a document is read
    /// through `referenced_uses` and not through its own paths.
    pub(crate) referenced: bool,
    /// The paths of the components in a referenced document that the root
    /// document's kept operations reach, such as `/components/parameters/N`.
    pub(crate) referenced_uses: Vec<String>,
    /// The filters the run applies. An operation they remove generates no
    /// query struct, so nothing checks its parameters.
    pub(crate) filters: crate::config::OutputOptions,
}

struct Sweep<'a> {
    document: &'a str,
    /// The components of other documents the kept operations reach.
    external: Vec<(String, String)>,
    warnings: Vec<Warning>,
    problems: Diagnostics,
    run: &'a Run,
    /// The depth of the operation, or path item, that the run's filters remove,
    /// while the walk is inside it.
    removed_at: Option<usize>,
    /// The depth of the schema that `x-rust-type` replaces, while the walk is
    /// inside it. The custom type reads everything below that schema, so no
    /// generated code checks a constraint there.
    replaced_at: Option<usize>,
    /// The paths of the schemas that sit directly in a query parameter. When
    /// the server is generated, its query struct checks the constraints of such
    /// a schema, so the note about unchecked constraints does not apply to it.
    query_schemas: Vec<String>,
    /// The schemas that the lowering reads by type, from [`read_by_type`].
    /// Such a schema, and what it reaches along that read, replaces nothing
    /// even when it carries `x-rust-type`. An object with properties that a
    /// model reads is lowered as a model, which reads the extension again.
    read_by_type: Vec<ReadByType>,
    /// The paths of the property schemas that hold a same-document `$ref` and
    /// a `description` beside it. See [`wrap_described_refs`].
    described_refs: Vec<String>,
}

/// Check `value` against what the generator reads, and report every warning.
///
/// `run` states what the run generates and keeps, which decides whether a
/// query parameter's constraints are checked.
///
/// The paths this returns are for [`wrap_described_refs`].
pub(crate) fn check(document: &str, value: &Value, run: &Run) -> Result<Inspected> {
    let sweep = inspect(document, value, run);
    report_warnings(document, &sweep.warnings);
    sweep.problems.into_result()?;
    return Ok(Inspected {
        described_refs: sweep.described_refs,
        external_uses: sweep.external,
    });
}

/// What the loader needs from an inspection.
pub(crate) struct Inspected {
    /// The paths of the property schemas that hold a `$ref` and a description
    /// beside it, for [`wrap_described_refs`].
    pub(crate) described_refs: Vec<String>,
    /// The components of other documents that the kept operations reach, each
    /// as the file and the component's path in it. The inspection of such a
    /// document reads those components as used.
    pub(crate) external_uses: Vec<(String, String)>,
}

/// Keep the `description` that sits beside a property's `$ref`.
///
/// OpenAPI 3.0 ignores every keyword beside a `$ref`, and the typed parse drops
/// them. A description changes no value and no type, and its loss leaves a
/// generated field with no documentation. So each such property becomes the
/// form OpenAPI 3.0 gives for this, a one-member `allOf` with the description
/// beside it. Any other keyword beside the `$ref` stays ignored.
///
/// `paths` comes from [`check`] on the same `value`.
pub(crate) fn wrap_described_refs(value: &mut Value, paths: &[String]) {
    for path in paths {
        let Some(node) = node_at(value, path) else {
            continue;
        };
        let (Some(reference), Some(description)) = (node.get("$ref").cloned(), node.get("description").cloned()) else {
            continue;
        };
        let mut member = serde_yaml::Mapping::new();
        member.insert(Value::from("$ref"), reference);
        let mut wrapper = serde_yaml::Mapping::new();
        wrapper.insert(Value::from("allOf"), Value::Sequence(vec![Value::Mapping(member)]));
        wrapper.insert(Value::from("description"), description);
        *node = Value::Mapping(wrapper);
    }
}

/// The node a path from this sweep names.
///
/// A key is looked up by its text. The sweep writes a numeric response code as
/// text too, and YAML reads `200:` as a number, so a token that no text key
/// holds is tried as the text of a number key.
fn node_at<'a>(value: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    let mut node = value;
    for token in path.split('/').skip(1) {
        let token = token.replace("~1", "/").replace("~0", "~");
        node = match node {
            Value::Mapping(mapping) => {
                if mapping.contains_key(token.as_str()) {
                    mapping.get_mut(token.as_str())?
                } else {
                    mapping.iter_mut().find_map(|(key, child)| {
                        return matches!(key, Value::Number(key) if key.to_string() == token).then_some(child);
                    })?
                }
            }
            Value::Sequence(sequence) => sequence.get_mut(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    return Some(node);
}

fn inspect<'a>(document: &'a str, value: &Value, run: &'a Run) -> Sweep<'a> {
    let by_type = read_by_type(value, run);
    let mut sweep = Sweep {
        document,
        external: by_type.external,
        warnings: Vec::new(),
        problems: Diagnostics::new(),
        run,
        removed_at: None,
        replaced_at: None,
        query_schemas: Vec::new(),
        read_by_type: by_type.schemas,
        described_refs: Vec::new(),
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
        // An operation the filters remove, or a path item with no operation
        // left, generates no query struct, so nothing checks its parameters.
        let removed_here = self.removed_at.is_none()
            && match context {
                Context::Operation => self.run_removes(value),
                Context::PathItem => !OPERATION_KEYS.iter().any(|method| {
                    return value
                        .get(*method)
                        .is_some_and(|operation| return !self.run_removes(operation));
                }),
                _ => false,
            };
        if removed_here {
            self.removed_at = Some(depth);
        }
        let replaces_here = matches!(context, Context::Schema | Context::PropertySchema)
            && value.get("x-rust-type").is_some()
            && self.replaced_at.is_none()
            && !self.is_read_by_type(path);
        if replaces_here {
            self.replaced_at = Some(depth);
        }
        // This schema's own notes still apply when it is the replacing one;
        // only what lies below a replacing schema is unlowered, and even there
        // a schema the lowering reads by type is lowered.
        let below_replacement = self.replaced_at.is_some_and(|at| return at < depth) && !self.is_read_by_type(path);
        if context == Context::Parameter
            && value.get("in").and_then(Value::as_str) == Some("query")
            && self.removed_at.is_none()
        {
            self.query_schemas.push(pointer(path, "schema"));
        }
        // The loader keeps this description, so it is not an ignored sibling.
        // A cross-file `$ref` does not resolve at a property, so nothing is kept
        // for one.
        let described = reference
            && context == Context::PropertySchema
            && value.get("description").is_some_and(Value::is_string)
            && value
                .get("$ref")
                .and_then(Value::as_str)
                .is_some_and(|target| return target.starts_with("#/"));
        if described {
            self.described_refs.push(path.to_owned());
        }
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
            if reference && context != Context::PathItem && !(described && key == "description") {
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
                // Below a replaced schema no code reads the feature, so a note
                // that it is not implemented would mislead; the key is still
                // checked and walked.
                if let Handling::Unsupported(reason) = field.handling
                    && child.as_bool() != Some(false)
                    && !below_replacement
                {
                    self.warn(&at, reason);
                }
                self.value_notes(context, &key, child, &at, below_replacement);
            }
            self.walk(child, field.traversal, &at, depth + 1);
        }
        // The replacing schema's own notes still apply: a constraint written on
        // it reaches no type, and the lowering reports that as an error.
        if replaces_here {
            self.replaced_at = None;
        }
        if !reference {
            self.object_notes(value, context, path);
        }
        if removed_here {
            self.removed_at = None;
        }
    }

    /// Whether the lowering reads the schema at `path` by type and not through
    /// `x-rust-type`: the schema is one of `read_by_type`, or lies below one
    /// along the segments that read follows.
    fn is_read_by_type(&self, path: &str) -> bool {
        return self.read_by_type.iter().any(|read| return read.covers(path));
    }

    /// Whether the run's filters remove this operation.
    fn run_removes(&self, operation: &Value) -> bool {
        return run_removes(self.run, operation);
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

    fn value_notes(&mut self, context: Context, key: &str, value: &Value, path: &str, below_replacement: bool) {
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
                // A `oneOf` asks for exactly one match. Two members that read the
                // same, apart from their descriptions, match the same values, so
                // the document admits no value of that shape. This is a fact
                // about the document, so it holds under `x-rust-type` too, where
                // the custom type and not generated code reads the value.
                "oneOf" if value.as_sequence().is_some_and(repeats_a_member) => {
                    self.warn(
                        path,
                        "two members of this oneOf are the same schema, so no value of that shape matches exactly one of them as the document requires",
                    );
                }
                "default" if value.is_null() && !below_replacement => {
                    self.warn(
                        path,
                        "the parser discards a null default, so an absent property does not receive explicit null",
                    );
                }
                "default" if context == Context::Schema && !below_replacement => {
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
        // Below a replaced schema nothing is lowered, so a note about what the
        // lowering would not enforce there would mislead. The replacing schema's
        // own notes stay, and so do those of a schema the lowering reads by type
        // through the replacement, as a multipart lowering reads a sent field.
        let lowered = self.replaced_at.is_none() || self.is_read_by_type(path);
        if lowered && value.get("x-rust-type").is_some() && value.get("allOf").is_some() {
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
            if lowered && crate::lower::default::unsupported_nullable_default(&schema) {
                self.warn(
                    &pointer(path, "default"),
                    "this nullable default has no supported Rust literal and is ignored",
                );
            }
            if lowered
                && crate::lower::constraints::unsupported_nullable_constraints(&schema)
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
        if context == Context::Schema
            && lowered
            && CONSTRAINT_KEYS.iter().any(|key| return value.get(*key).is_some())
            && !self.checked_query_schema(value, path)
            && !unsigned_type_holds_the_bound(value)
        {
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
        if lowered && matches!(kind, Some("number" | "boolean")) && value.get("enum").is_some() {
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
            if lowered && !handled {
                self.warn(
                    &pointer(path, "format"),
                    "this format is not implemented and the base type is used",
                );
            }
        }
    }
}

impl Sweep<'_> {
    /// Whether `value` is the schema of a query parameter that the generated
    /// query struct checks on the way in. A client only writes a query, so a
    /// run with no server checks nothing.
    ///
    /// The query field takes the constraints that `constraints_of` reads, so the
    /// same read decides here. A keyword it drops, such as `minLength` on an
    /// integer, keeps the note.
    fn checked_query_schema(&self, value: &Value, path: &str) -> bool {
        return self.run.server
            && self.query_schemas.iter().any(|schema| return schema == path)
            && serde_yaml::from_value::<openapiv3::Schema>(value.clone())
                .is_ok_and(|schema| return crate::lower::constraints::constraints_of(&schema).is_some());
    }
}

/// Whether the only constraint on an integer is `minimum: 0`.
///
/// Such a schema becomes an unsigned Rust type, and that type refuses every
/// value the bound refuses. So the bound holds at every use, and an alias or
/// an array item needs no check.
fn unsigned_type_holds_the_bound(value: &Value) -> bool {
    let only_minimum = CONSTRAINT_KEYS
        .iter()
        .all(|key| return (*key == "minimum") == value.get(*key).is_some());
    return only_minimum
        && value.get("type").and_then(Value::as_str) == Some("integer")
        && value.get("minimum").and_then(Value::as_i64) == Some(0)
        && value.get("x-rust-type").is_none();
}

/// Whether two of `members` are the same schema once their descriptions are
/// set aside.
fn repeats_a_member(members: &serde_yaml::Sequence) -> bool {
    let shapes: Vec<Value> = members
        .iter()
        .map(|member| {
            let mut shape = member.clone();
            strip_descriptions(&mut shape);
            return shape;
        })
        .collect();
    return shapes
        .iter()
        .enumerate()
        .any(|(index, shape)| return shapes.iter().take(index).any(|earlier| return earlier == shape));
}

/// The keys of a schema whose value is one schema.
const SCHEMA_KEYS: &[&str] = &["items", "additionalProperties", "not"];
/// The keys of a schema whose value is a list of schemas.
const SCHEMA_LIST_KEYS: &[&str] = &["allOf", "oneOf", "anyOf"];

/// Remove the description of `schema` and of each schema written inside it.
/// A property name is data and may read `description`, and so may a key inside
/// `example`, `default`, or an `enum` value, so only the places that hold a
/// schema are entered.
fn strip_descriptions(schema: &mut Value) {
    let Some(mapping) = schema.as_mapping_mut() else {
        return;
    };
    mapping.remove("description");
    for key in SCHEMA_KEYS {
        if let Some(inner) = mapping.get_mut(*key) {
            strip_descriptions(inner);
        }
    }
    for key in SCHEMA_LIST_KEYS {
        if let Some(Value::Sequence(members)) = mapping.get_mut(*key) {
            members.iter_mut().for_each(strip_descriptions);
        }
    }
    if let Some(Value::Mapping(properties)) = mapping.get_mut("properties") {
        properties.values_mut().for_each(strip_descriptions);
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
    use crate::config::OutputOptions;

    fn inspect_yaml(yaml: &str) -> Sweep<'static> {
        return inspect_yaml_for(yaml, true, OutputOptions::default());
    }

    fn inspect_yaml_for(yaml: &str, server: bool, filters: OutputOptions) -> Sweep<'static> {
        return inspect_yaml_run(
            yaml,
            Run {
                server,
                operations: true,
                referenced: false,
                referenced_uses: Vec::new(),
                filters,
            },
        );
    }

    fn inspect_yaml_run(yaml: &str, run: Run) -> Sweep<'static> {
        let value = serde_yaml::from_str(yaml).expect("valid YAML");
        return inspect("spec.yaml", &value, Box::leak(Box::new(run)));
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
    fn a_description_beside_a_property_reference_is_kept_and_not_reported() {
        const HOLDER: &str = "/components/schemas/Holder/properties/a";
        let holder = |property: &str| {
            return format!("components: {{schemas: {{Holder: {{type: object, properties: {{a: {property}}}}}}}}}");
        };
        for (yaml, kept, ignored) in [
            (
                holder("{$ref: '#/components/schemas/Other', description: note}"),
                true,
                vec![],
            ),
            // Any other keyword beside the `$ref` stays ignored.
            (
                holder("{$ref: '#/components/schemas/Other', description: note, nullable: true}"),
                true,
                vec!["nullable"],
            ),
            (
                holder("{$ref: '#/components/schemas/Other', nullable: true}"),
                false,
                vec!["nullable"],
            ),
            // A cross-file `$ref` does not resolve at a property.
            (
                holder("{$ref: 'other.yaml#/components/schemas/Other', description: note}"),
                false,
                vec!["description"],
            ),
            (holder("{$ref: '#/components/schemas/Other'}"), false, vec![]),
        ] {
            let mut value: Value = serde_yaml::from_str(&yaml).expect("yaml");
            let run = Run::default();
            let sweep = inspect("openapi.yaml", &value, &run);
            assert!(sweep.problems.is_empty(), "{yaml}");
            let reported: Vec<String> = sweep
                .warnings
                .iter()
                .map(|warning| return warning.path.clone())
                .collect();
            let expected: Vec<String> = ignored.iter().map(|key| return pointer(HOLDER, key)).collect();
            assert_eq!(reported, expected, "{yaml}");

            wrap_described_refs(&mut value, &sweep.described_refs);
            let property = node_at(&mut value, HOLDER).expect("the property");
            assert_eq!(property.get("allOf").is_some(), kept, "{yaml}");
            if kept {
                let wrapped: Value =
                    serde_yaml::from_str("{allOf: [{$ref: '#/components/schemas/Other'}], description: note}")
                        .expect("yaml");
                assert_eq!(*property, wrapped, "{yaml}");
            }
        }
    }

    #[test]
    fn a_path_through_a_numeric_response_code_and_an_array_finds_its_node() {
        let mut value: Value = serde_yaml::from_str(
            "paths: {/a~b: {get: {responses: {200: {content: {application/json: {schema: {allOf: [{type: string}]}}}}}}}}",
        )
        .expect("yaml");
        let path = "/paths/~1a~0b/get/responses/200/content/application~1json/schema/allOf/0/type";
        assert_eq!(
            node_at(&mut value, path).and_then(|node| return node.as_str()),
            Some("string")
        );
        assert!(node_at(&mut value, "/paths/missing").is_none());
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
        }
    }

    /// A union matches by the Rust types of its members, and the notes on an
    /// unchecked constraint already name every place where that read differs
    /// from the document. So a union carries no note of its own.
    #[test]
    fn notes_about_the_lowering_are_quiet_below_a_replaced_schema() {
        for (item, message) in [
            (
                "{type: string, format: date, nullable: true, maxLength: 3}",
                "constraints on this nullable value type are not enforced",
            ),
            (
                "{x-rust-type: 'crate::Inner', allOf: [{type: string, maxLength: 3}]}",
                "constraints inherited through allOf are not enforced for x-rust-type",
            ),
            (
                "{type: boolean, enum: [true]}",
                "number and boolean enum restrictions are not enforced",
            ),
            (
                "{type: string, xml: {name: item}}",
                "XML serialization is not implemented",
            ),
            (
                "{type: string, format: custom}",
                "this format is not implemented and the base type is used",
            ),
            (
                "{type: string, nullable: true, default: null}",
                "the parser discards a null default",
            ),
        ] {
            let below = inspect_yaml(&format!(
                "components: {{schemas: {{Stamp: {{x-rust-type: 'crate::Stamp', type: array, items: {item}}}}}}}"
            ));
            assert!(below.problems.is_empty(), "{item}");
            assert!(
                !below
                    .warnings
                    .iter()
                    .any(|warning| return warning.message.contains(message)),
                "{item}: {:?}",
                below.warnings
            );
            // A multipart body reads the component and its properties by type,
            // so the note stays there even under the extension, also when the
            // body is reached through a chain of component request bodies. A
            // form-encoded body reuses the model, which reads the extension.
            let schemas = format!(
                "schemas: {{Form: {{x-rust-type: 'crate::Form', type: object, properties: {{field: {item}}}}}}}"
            );
            let multipart = inspect_yaml(&format!(
                "paths: {{/a: {{post: {{requestBody: {{$ref: '#/components/requestBodies/Outer'}}, responses: {{}}}}}}}}\ncomponents: {{requestBodies: {{Outer: {{$ref: '#/components/requestBodies/Inner'}}, Inner: {{required: true, content: {{multipart/form-data: {{schema: {{$ref: '#/components/schemas/Form'}}}}}}}}}}, {schemas}}}"
            ));
            assert!(
                multipart
                    .warnings
                    .iter()
                    .any(|warning| return warning.message.contains(message)),
                "{item}: {:?}",
                multipart.warnings
            );
            // The media type is read without case and parameters, as the body
            // lowering reads it.
            let spelled = inspect_yaml(&format!(
                "paths: {{/a: {{post: {{requestBody: {{required: true, content: {{'Multipart/Form-Data; boundary=x': {{schema: {{$ref: '#/components/schemas/Form'}}}}}}}}, responses: {{}}}}}}}}\ncomponents: {{{schemas}}}"
            ));
            assert!(
                spelled
                    .warnings
                    .iter()
                    .any(|warning| return warning.message.contains(message)),
                "{item}: {:?}",
                spelled.warnings
            );
            let encoded = inspect_yaml(&format!(
                "paths: {{/a: {{post: {{requestBody: {{required: true, content: {{application/x-www-form-urlencoded: {{schema: {{$ref: '#/components/schemas/Form'}}}}}}}}, responses: {{}}}}}}}}\ncomponents: {{{schemas}}}"
            ));
            assert!(
                !encoded
                    .warnings
                    .iter()
                    .any(|warning| return warning.message.contains(message)),
                "{item}: {:?}",
                encoded.warnings
            );
            // The same item under a plain array is lowered, so it keeps the note.
            let lowered = inspect_yaml(&format!(
                "components: {{schemas: {{Stamps: {{type: array, items: {item}}}}}}}"
            ));
            assert!(
                lowered
                    .warnings
                    .iter()
                    .any(|warning| return warning.message.contains(message)),
                "{item}: {:?}",
                lowered.warnings
            );
        }
    }

    /// The resolver looks up to `MAX_REF_DEPTH` components, the final one
    /// included. A chain that needs exactly that many lookups resolves, and one
    /// more does not, so the inspection must read the same chains. `links` is
    /// the number of component request bodies that are themselves references;
    /// the final body is one more lookup.
    #[test]
    fn a_request_body_chain_at_the_depth_limit_is_still_read_by_type() {
        const NOTE: &str = "constraints are enforced only at supported field uses";
        for (links, read) in [
            (crate::loader::MAX_REF_DEPTH - 1, true),
            (crate::loader::MAX_REF_DEPTH, false),
        ] {
            let mut bodies: Vec<String> = (1..=links)
                .map(|index| {
                    let target = if index == links {
                        "Final".to_owned()
                    } else {
                        format!("Body{}", index + 1)
                    };
                    return format!("Body{index}: {{$ref: '#/components/requestBodies/{target}'}}");
                })
                .collect();
            bodies.push("Final: {required: true, content: {multipart/form-data: {schema: {$ref: '#/components/schemas/Form'}}}}".to_owned());
            let yaml = format!(
                "paths: {{/a: {{post: {{requestBody: {{$ref: '#/components/requestBodies/Body1'}}, responses: {{}}}}}}}}\ncomponents: {{requestBodies: {{{}}}, schemas: {{Form: {{x-rust-type: 'crate::Form', type: object, properties: {{field: {{type: array, items: {{type: string, maxLength: 3}}}}}}}}}}}}",
                bodies.join(", ")
            );
            let sweep = inspect_yaml(&yaml);
            assert_eq!(
                sweep
                    .warnings
                    .iter()
                    .any(|warning| return warning.message.contains(NOTE)),
                read,
                "{links} links: {:?}",
                sweep.warnings
            );
        }
    }

    #[test]
    fn a_union_carries_no_note_of_its_own() {
        for schema in [
            "{oneOf: [{type: string}, {type: integer}]}",
            "{anyOf: [{type: string}, {type: integer}]}",
            "{x-rust-type: 'crate::Stamp', oneOf: [{type: string}, {type: integer}]}",
        ] {
            let sweep = inspect_yaml(&format!("components: {{schemas: {{Widget: {schema}}}}}"));
            assert!(sweep.problems.is_empty(), "{schema}");
            assert!(sweep.warnings.is_empty(), "{schema}: {:?}", sweep.warnings);
        }
        // A constraint a member holds that no code checks still has its note.
        let sweep =
            inspect_yaml("components: {schemas: {Widget: {oneOf: [{type: string, maxLength: 3}, {type: integer}]}}}");
        assert_eq!(sweep.warnings.len(), 1, "{:?}", sweep.warnings);
        assert!(sweep.warnings[0].path.ends_with("/oneOf/0"), "{:?}", sweep.warnings);
    }

    #[test]
    fn a_one_of_with_two_identical_members_is_reported() {
        const NOTE: &str = "two members of this oneOf are the same schema";
        for (schema, noted) in [
            // Both members are `date-time` strings, so no value matches exactly one.
            (
                "{oneOf: [{type: string, format: date}, {type: string, format: date-time}, {type: string, format: date-time}]}",
                true,
            ),
            // A description does not tell two members apart, at any depth.
            (
                "{oneOf: [{type: array, items: {type: string, description: a}}, {type: array, items: {type: string, description: b}}]}",
                true,
            ),
            (
                "{oneOf: [{type: object, properties: {p: {type: string, description: a}}}, {type: object, properties: {p: {type: string}}}]}",
                true,
            ),
            // A property named `description` is data, and so is an example.
            (
                "{oneOf: [{type: object, properties: {description: {type: string}}}, {type: object, properties: {description: {type: integer}}}]}",
                false,
            ),
            (
                "{oneOf: [{type: string, example: a}, {type: string, example: b}]}",
                false,
            ),
            // The fact holds whichever type reads the value.
            (
                "{x-rust-type: 'crate::Stamp', oneOf: [{type: string}, {type: string}]}",
                true,
            ),
            (
                "{oneOf: [{type: string, description: a}, {type: string, description: b}]}",
                true,
            ),
            (
                "{oneOf: [{$ref: '#/components/schemas/A'}, {$ref: '#/components/schemas/A'}]}",
                true,
            ),
            (
                "{oneOf: [{type: string, format: date}, {type: string, format: date-time}]}",
                false,
            ),
            // `anyOf` accepts more than one match, so a repeat changes nothing.
            ("{anyOf: [{type: string}, {type: string}]}", false),
        ] {
            let sweep = inspect_yaml(&format!("components: {{schemas: {{Widget: {schema}}}}}"));
            assert!(sweep.problems.is_empty(), "{schema}");
            assert_eq!(
                sweep
                    .warnings
                    .iter()
                    .any(|warning| return warning.message.contains(NOTE)),
                noted,
                "{schema}: {:?}",
                sweep.warnings
            );
        }
    }

    #[test]
    fn the_unchecked_constraint_note_is_only_at_uses_that_no_code_checks() {
        const NOTE: &str = "constraints are enforced only at supported field uses";
        let parameter = |location: &str, schema: &str| {
            return format!(
                "paths: {{/a: {{get: {{parameters: [{{name: n, in: {location}, schema: {schema}}}], responses: {{}}}}}}}}"
            );
        };
        let component = |schema: &str| return format!("components: {{schemas: {{Widget: {schema}}}}}");
        for (yaml, noted) in [
            // The query struct checks a scalar on the way in.
            (parameter("query", "{type: string, maxLength: 3}"), false),
            (parameter("query", "{type: integer, minimum: 1, maximum: 9}"), false),
            // The struct checks the keywords of the array itself.
            (
                parameter("query", "{type: array, maxItems: 3, items: {type: string}}"),
                false,
            ),
            // No other parameter location has a check.
            (parameter("header", "{type: string, maxLength: 3}"), true),
            // The constraint on an item of a query array has no check.
            (
                parameter("query", "{type: array, items: {type: string, maxLength: 3}}"),
                true,
            ),
            // An unsigned type refuses what `minimum: 0` refuses, at every use.
            (component("{type: integer, minimum: 0}"), false),
            (
                component("{type: array, items: {type: integer, format: int32, minimum: 0}}"),
                false,
            ),
            (component("{type: integer, minimum: 1}"), true),
            (component("{type: integer, minimum: 0, maximum: 9}"), true),
            (component("{type: number, minimum: 0}"), true),
            (component("{type: string, pattern: '^a$'}"), true),
            // Below a custom type no code is generated, so no check is missing;
            // a constraint on the replacing schema itself keeps its note.
            (
                component("{x-rust-type: 'crate::Stamp', oneOf: [{type: string, pattern: '^a$'}, {type: integer}]}"),
                false,
            ),
            (
                component("{x-rust-type: 'crate::Stamp', type: array, items: {type: string, maxLength: 3}}"),
                false,
            ),
            (
                component("{x-rust-type: 'crate::Stamp', type: string, pattern: '^a$'}"),
                true,
            ),
            // The parameter lowering reads the type and not the extension, so a
            // query schema with `x-rust-type` replaces nothing, and its items
            // keep their note. The same holds through a `$ref`, and for the
            // inline schema of a body.
            (
                parameter(
                    "query",
                    "{x-rust-type: 'crate::Stamp', type: array, items: {type: string, maxLength: 3}}",
                ),
                true,
            ),
            (
                format!(
                    "{}\ncomponents: {{schemas: {{Stamp: {{x-rust-type: 'crate::Stamp', type: array, items: {{type: string, maxLength: 3}}}}}}}}",
                    parameter("query", "{$ref: '#/components/schemas/Stamp'}")
                ),
                true,
            ),
            (
                "paths: {/a: {post: {requestBody: {content: {application/json: {schema: {x-rust-type: 'crate::Stamp', type: array, items: {type: string, maxLength: 3}}}}}, responses: {}}}}".to_owned(),
                true,
            ),
            // The chain through an alias, a form body, and a one-member `allOf`
            // in a body is read by type too.
            (
                format!(
                    "{}\ncomponents: {{schemas: {{Alias: {{$ref: '#/components/schemas/Stamp'}}, Stamp: {{x-rust-type: 'crate::Stamp', type: array, items: {{type: string, maxLength: 3}}}}}}}}",
                    parameter("query", "{$ref: '#/components/schemas/Alias'}")
                ),
                true,
            ),
            (
                "paths: {/a: {post: {requestBody: {content: {application/json: {schema: {allOf: [{x-rust-type: 'crate::Stamp', type: array, items: {type: string, maxLength: 3}}]}}}}, responses: {}}}}".to_owned(),
                true,
            ),
            // A sibling component whose name starts the same is not below the
            // parameter's schema.
            (
                format!(
                    "{}\ncomponents: {{schemas: {{Stamp: {{type: string}}, StampExtra: {{x-rust-type: 'crate::Extra', type: array, items: {{type: string, maxLength: 3}}}}}}}}",
                    parameter("query", "{$ref: '#/components/schemas/Stamp'}")
                ),
                false,
            ),
            // A model property reads the extension, so below it the note is quiet.
            (
                component("{type: object, properties: {stamp: {x-rust-type: 'crate::Stamp', type: array, items: {type: string, maxLength: 3}}}}"),
                false,
            ),
        ] {
            let sweep = inspect_yaml(&yaml);
            assert!(sweep.problems.is_empty(), "{yaml}: {:?}", sweep.problems);
            assert_eq!(
                sweep
                    .warnings
                    .iter()
                    .any(|warning| return warning.message.contains(NOTE)),
                noted,
                "{yaml}: {:?}",
                sweep.warnings,
            );
        }

        // A client writes a query and reads nothing back from it, so a run with
        // no server checks no query constraint.
        let client_only = inspect_yaml_for(
            &parameter("query", "{type: string, maxLength: 3}"),
            false,
            OutputOptions::default(),
        );
        assert!(
            client_only
                .warnings
                .iter()
                .any(|warning| return warning.message.contains(NOTE))
        );

        // The query field reads the same keywords as the lowering: a keyword the
        // type cannot hold keeps the note, and a custom type on a query parameter
        // changes nothing, because the query lowering does not read it.
        for (schema, noted) in [
            ("{type: integer, minLength: 3}", true),
            ("{type: string, maxLength: 3, x-rust-type: 'crate::Code'}", false),
        ] {
            let sweep = inspect_yaml(&parameter("query", schema));
            assert_eq!(
                sweep
                    .warnings
                    .iter()
                    .any(|warning| return warning.message.contains(NOTE)),
                noted,
                "{schema}: {:?}",
                sweep.warnings
            );
        }

        // An operation the filters remove generates no query struct. A path item
        // keeps its own parameters while one of its operations stays.
        let filtered = |yaml: &str, filters: OutputOptions| {
            return inspect_yaml_for(yaml, true, filters)
                .warnings
                .iter()
                .any(|warning| return warning.message.contains(NOTE));
        };
        let tagged = "paths: {/a: {get: {operationId: getA, tags: [internal], parameters: [{name: n, in: query, schema: {type: string, maxLength: 3}}], responses: {}}}}";
        let shared = "paths: {/a: {parameters: [{name: n, in: query, schema: {type: string, maxLength: 3}}], get: {operationId: getA, tags: [internal], responses: {}}, post: {operationId: postA, responses: {}}}}";
        let exclude = OutputOptions {
            exclude_tags: vec!["internal".to_owned()],
            ..OutputOptions::default()
        };
        let exclude_both = OutputOptions {
            exclude_operation_ids: vec!["getA".to_owned(), "postA".to_owned()],
            ..OutputOptions::default()
        };
        assert!(!filtered(tagged, OutputOptions::default()));
        assert!(filtered(tagged, exclude.clone()));
        assert!(!filtered(shared, exclude.clone()));
        assert!(filtered(shared, exclude_both));

        // A parameter in a removed operation reads nothing, so the custom type
        // model it names keeps its replacement and loses the note.
        let referenced = "paths: {/a: {get: {operationId: getA, tags: [internal], parameters: [{name: n, in: query, schema: {$ref: '#/components/schemas/Stamp'}}], responses: {}}}}\ncomponents: {schemas: {Stamp: {x-rust-type: 'crate::Stamp', type: array, items: {type: string, maxLength: 3}}}}";
        assert!(filtered(referenced, OutputOptions::default()));
        assert!(!filtered(referenced, exclude));
    }

    /// What the lowering reads by type follows from what the run lowers, not
    /// from where a schema sits in the document.
    #[test]
    fn a_schema_is_read_by_type_only_where_a_lowered_operation_reads_it() {
        const NOTE: &str = "constraints are enforced only at supported field uses";
        let stamp = "{x-rust-type: 'crate::Stamp', type: array, items: {type: string, maxLength: 3}}";
        let noted = |yaml: &str, run: Run| {
            return inspect_yaml_run(yaml, run)
                .warnings
                .iter()
                .any(|warning| return warning.message.contains(NOTE));
        };
        let operations = Run {
            operations: true,
            ..Run::default()
        };
        let models_only = Run::default();
        let referenced = Run {
            referenced: true,
            operations: true,
            referenced_uses: vec!["/components/parameters/N".to_owned()],
            ..Run::default()
        };
        let referenced_elsewhere = Run {
            referenced: true,
            operations: true,
            referenced_uses: vec!["/components/parameters/Other".to_owned()],
            ..Run::default()
        };

        // A models-only run lowers no parameter, so the custom type is an alias
        // and nothing else.
        let query = format!(
            "paths: {{/a: {{get: {{parameters: [{{name: n, in: query, schema: {stamp}}}], responses: {{}}}}}}}}"
        );
        assert!(noted(&query, operations.clone()));
        assert!(!noted(&query, models_only.clone()));

        // A path item's parameter that the operation overrides is not read.
        let overridden = format!(
            "paths: {{/a: {{parameters: [{{name: n, in: query, schema: {stamp}}}], get: {{parameters: [{{name: n, in: query, schema: {{type: string}}}}], responses: {{}}}}}}}}"
        );
        let inherited = format!(
            "paths: {{/a: {{parameters: [{{name: n, in: query, schema: {stamp}}}], get: {{parameters: [{{name: n, in: header, schema: {{type: string}}}}], responses: {{}}}}}}}}"
        );
        assert!(!noted(&overridden, operations.clone()));
        assert!(noted(&inherited, operations.clone()));

        // A component parameter is read only when a kept operation names it,
        // or when the document is one that another document references and
        // that document's operations reach this very component.
        let unused = format!("paths: {{}}\ncomponents: {{parameters: {{N: {{name: n, in: query, schema: {stamp}}}}}}}");
        let named = format!(
            "paths: {{/a: {{get: {{parameters: [{{$ref: '#/components/parameters/N'}}], responses: {{}}}}}}}}\ncomponents: {{parameters: {{N: {{name: n, in: query, schema: {stamp}}}}}}}"
        );
        assert!(!noted(&unused, operations.clone()));
        assert!(noted(&unused, referenced.clone()));
        assert!(!noted(&unused, referenced_elsewhere));
        assert!(!noted(
            &unused,
            Run {
                operations: false,
                ..referenced
            }
        ));
        assert!(noted(&named, operations.clone()));

        // An inline response body is read by type as an inline request body is.
        let response = format!(
            "paths: {{/a: {{get: {{responses: {{'200': {{description: ok, content: {{application/json: {{schema: {stamp}}}}}}}}}}}}}}}"
        );
        assert!(noted(&response, operations.clone()));

        // A referenced document's own operations are not lowered by the run.
        let referenced_run = Run {
            referenced: true,
            operations: true,
            ..Run::default()
        };
        assert!(!noted(&query, referenced_run));

        // A header is overridden without case, and the headers the framework
        // owns are never read.
        let header_override = format!(
            "paths: {{/a: {{parameters: [{{name: X-Token, in: header, schema: {stamp}}}], get: {{parameters: [{{name: x-token, in: header, schema: {{type: string}}}}], responses: {{}}}}}}}}"
        );
        let reserved = format!(
            "paths: {{/a: {{get: {{parameters: [{{name: Authorization, in: header, schema: {stamp}}}], responses: {{}}}}}}}}"
        );
        let plain_header = format!(
            "paths: {{/a: {{get: {{parameters: [{{name: X-Token, in: header, schema: {stamp}}}], responses: {{}}}}}}}}"
        );
        assert!(!noted(&header_override, operations.clone()));
        assert!(!noted(&reserved, operations.clone()));
        assert!(noted(&plain_header, operations.clone()));

        // A multipart body reads the properties the request sends, and a
        // `readOnly` one is skipped before its type is read.
        let multipart = |read_only: &str| {
            return format!(
                "paths: {{/a: {{post: {{requestBody: {{required: true, content: {{multipart/form-data: {{schema: {{$ref: '#/components/schemas/Form'}}}}}}}}, responses: {{}}}}}}}}\ncomponents: {{schemas: {{Form: {{x-rust-type: 'crate::Form', type: object, properties: {{tags: {{{read_only}type: array, items: {{type: string, maxLength: 3}}}}}}}}}}}}"
            );
        };
        assert!(noted(&multipart(""), operations.clone()));
        assert!(!noted(&multipart("readOnly: true, "), operations.clone()));

        // A field that names a `readOnly` schema is skipped the same way, so a
        // custom type array behind it keeps its replacement.
        let through_reference = |read_only: &str| {
            return format!(
                "paths: {{/a: {{post: {{requestBody: {{required: true, content: {{multipart/form-data: {{schema: {{$ref: '#/components/schemas/Form'}}}}}}}}, responses: {{}}}}}}}}\ncomponents: {{schemas: {{Served: {{{read_only}x-rust-type: 'crate::Served', type: array, items: {{type: string, maxLength: 3}}}}, Form: {{type: object, properties: {{tags: {{$ref: '#/components/schemas/Served'}}}}}}}}}}"
            );
        };
        assert!(noted(&through_reference(""), operations.clone()));
        assert!(!noted(&through_reference("readOnly: true, "), operations.clone()));

        // The body lowering reads the first entry of each kind it supports and
        // ignores the rest, so a second multipart entry and an XML entry are
        // not read.
        let second_multipart = format!(
            "paths: {{/a: {{post: {{requestBody: {{required: true, content: {{multipart/form-data: {{schema: {{type: object}}}}, 'multipart/form-data; boundary=x': {{schema: {{$ref: '#/components/schemas/Form'}}}}}}}}, responses: {{}}}}}}}}\ncomponents: {{schemas: {{Form: {{x-rust-type: 'crate::Form', type: object, properties: {{tags: {stamp}}}}}}}}}"
        );
        let xml = format!(
            "paths: {{/a: {{post: {{requestBody: {{required: true, content: {{application/json: {{schema: {{type: object}}}}, application/xml: {{schema: {stamp}}}}}}}, responses: {{}}}}}}}}"
        );
        assert!(!noted(&second_multipart, operations.clone()));
        assert!(!noted(&xml, operations.clone()));

        // A `$ref` into another document is handed to that document's
        // inspection as a use, instead of being read here.
        let external = read_by_type(
            &serde_yaml::from_str(
                "paths: {/a: {get: {parameters: [{$ref: 'shared.yaml#/components/parameters/N'}], responses: {}}}}",
            )
            .expect("yaml"),
            &operations,
        );
        assert_eq!(
            external.external,
            vec![("shared.yaml".to_owned(), "/components/parameters/N".to_owned())]
        );
    }

    /// The replacing schema is lowered, so its own notes stay, those on its
    /// keys included. Only what lies below it is unlowered.
    #[test]
    fn the_replacing_schema_keeps_the_notes_on_its_own_keys() {
        const NOTE: &str = "the parser discards a null default";
        let own = inspect_yaml(
            "components: {schemas: {Holder: {type: object, properties: {stamp: {x-rust-type: String, type: string, nullable: true, default: null}}}}}",
        );
        assert!(
            own.warnings.iter().any(|warning| return warning.message.contains(NOTE)),
            "{:?}",
            own.warnings
        );
        let below = inspect_yaml(
            "components: {schemas: {Stamp: {x-rust-type: 'crate::Stamp', type: array, items: {type: string, nullable: true, default: null}}}}",
        );
        assert!(
            !below
                .warnings
                .iter()
                .any(|warning| return warning.message.contains(NOTE)),
            "{:?}",
            below.warnings
        );
    }

    #[test]
    fn empty_closed_objects_have_no_map_warning() {
        let sweep = inspect_yaml("components: {schemas: {Empty: {type: object, additionalProperties: false}}}");
        assert!(sweep.problems.is_empty());
        assert!(sweep.warnings.is_empty());
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
