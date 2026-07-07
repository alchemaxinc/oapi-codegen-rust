//! Intermediate representation (IR) of generated Rust types.
//!
//! The schema mapping pass ([`crate::lower::schema`]) lowers OpenAPI schemas into this
//! IR; the emit pass ([`crate::emit`]) turns the IR into a token stream. Keeping
//! the two separate makes the mapping logic testable without touching token
//! generation, and keeps emission free of OpenAPI concerns.

use crate::naming::RustIdent;

/// A generated Rust source module: an ordered list of top-level items.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Module {
    /// Top-level type declarations, in deterministic emission order.
    pub items: Vec<Item>,
}

/// A top-level item declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A `struct` declaration.
    Struct(Struct),
    /// An `enum` declaration (string enum or union).
    Enum(Enum),
    /// A `type X = Y;` alias.
    Alias(Alias),
}

impl Item {
    /// The declared name of the item, used for stable ordering.
    pub fn name(&self) -> &str {
        let name = match self {
            Item::Struct(s) => s.name.logical(),
            Item::Enum(e) => e.name.logical(),
            Item::Alias(a) => a.name.logical(),
        };
        return name;
    }
}

/// A generated `struct`.
#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    /// Type name.
    pub name: RustIdent,
    /// Doc comment derived from the schema `description`.
    pub doc: Option<String>,
    /// Named fields, in deterministic order.
    pub fields: Vec<Field>,
    /// When set, the struct captures unknown keys into a flattened map of this
    /// element type (`additionalProperties`).
    pub additional_properties: Option<RustType>,
}

/// A single struct field.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Rust field identifier.
    pub name: RustIdent,
    /// `#[serde(rename = "...")]` value, when the wire name differs.
    pub rename: Option<String>,
    /// Doc comment derived from the property `description`.
    pub doc: Option<String>,
    /// Field type (already wrapped in `Option<..>` when optional).
    pub ty: RustType,
    /// Whether the property is required. Optional fields get
    /// `#[serde(skip_serializing_if = "Option::is_none")]`.
    pub required: bool,
}

/// A generated `enum`.
#[derive(Debug, Clone, PartialEq)]
pub struct Enum {
    /// Type name.
    pub name: RustIdent,
    /// Doc comment derived from the schema `description`.
    pub doc: Option<String>,
    /// The flavour of enum to emit.
    pub kind: EnumKind,
}

/// The flavour of a generated enum.
#[derive(Debug, Clone, PartialEq)]
pub enum EnumKind {
    /// A C-like string enum: unit variants mapped to wire strings.
    Strings(Vec<StringVariant>),
    /// A `#[serde(untagged)]` union over the given newtype variants.
    ///
    /// Untagged (rather than internally tagged) is used even when the OpenAPI
    /// schema has a discriminator: OpenAPI variant schemas typically carry the
    /// discriminator property themselves, which is incompatible with serde's
    /// internally-tagged representation.
    Union(Vec<UnionVariant>),
}

/// A unit variant of a string enum.
#[derive(Debug, Clone, PartialEq)]
pub struct StringVariant {
    /// Rust variant identifier.
    pub name: RustIdent,
    /// `#[serde(rename = "...")]` value, when the wire value differs.
    pub rename: Option<String>,
    /// Doc comment, if any.
    pub doc: Option<String>,
}

/// A newtype variant of a union enum.
#[derive(Debug, Clone, PartialEq)]
pub struct UnionVariant {
    /// Rust variant identifier.
    pub name: RustIdent,
    /// The wrapped type.
    pub ty: RustType,
}

/// A generated `type X = Y;` alias.
#[derive(Debug, Clone, PartialEq)]
pub struct Alias {
    /// Alias name.
    pub name: RustIdent,
    /// Doc comment, if any.
    pub doc: Option<String>,
    /// Aliased type.
    pub ty: RustType,
}

/// A Rust type expression usable in field/alias position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustType {
    /// `bool`.
    Bool,
    /// `i32`.
    I32,
    /// `i64`.
    I64,
    /// `f64`.
    F64,
    /// `String`.
    String,
    /// `serde_json::Value` (free-form / empty schema).
    Value,
    /// `chrono::NaiveDate` (`format: date`).
    Date,
    /// `chrono::DateTime<chrono::Utc>` (`format: date-time`).
    DateTime,
    /// `uuid::Uuid` (`format: uuid`).
    Uuid,
    /// `Vec<u8>` (`format: byte`/`binary`).
    Bytes,
    /// `Vec<T>`.
    Vec(Box<RustType>),
    /// `std::collections::HashMap<String, T>`.
    Map(Box<RustType>),
    /// `Option<T>`.
    Option(Box<RustType>),
    /// A reference to a named (generated or external) type.
    Named(String),
    /// A reference to a type from an import-mapped module: `module::Name`.
    External { module: String, name: String },
    /// A verbatim type expression from an `x-rust-type` extension.
    Verbatim(String),
}

impl RustType {
    /// Wrap this type in `Option<..>`.
    pub fn optional(self) -> RustType {
        return RustType::Option(Box::new(self));
    }

    /// Whether this type already has a `None`/empty representation and so does
    /// not need an `Option<..>` wrapper to express absence (`Vec`, `Map`).
    pub fn is_nullable_container(&self) -> bool {
        let nullable = matches!(self, RustType::Vec(_) | RustType::Map(_));
        return nullable;
    }
}

/// A request or response body: its Rust type plus the wire content type that
/// selects the axum extractor / response wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// The Rust type of the decoded body.
    pub ty: RustType,
    /// The content type that selects the extractor / response wrapper.
    pub kind: BodyKind,
}

/// The supported request/response content type for a [`Body`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// `application/json` (and `+json` / charset variants) → `axum::Json`.
    Json,
    /// `text/plain` → `String`.
    Text,
    /// `application/x-www-form-urlencoded` → `axum::Form`.
    Form,
}

/// A generated axum server interface: the `Api` trait plus the operations that
/// back its `Router`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Service {
    /// Operations in deterministic (document) order.
    pub operations: Vec<Operation>,
}

/// A single HTTP operation lowered for code generation.
#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    /// `Api` trait method name (`snake_case`).
    pub name: RustIdent,
    /// Per-operation response enum name (`<Name>Response`).
    pub response_enum: RustIdent,
    /// Doc comment derived from the operation `summary`/`description`.
    pub doc: Option<String>,
    /// Lowercase HTTP method verb (`get`, `post`, …), which is also the
    /// `axum::routing` helper name.
    pub method: String,
    /// Request path template, reused verbatim as the axum route (axum 0.8 and
    /// OpenAPI share the `/{name}` path-parameter syntax).
    pub path: String,
    /// Typed path parameters, in path order.
    pub path_params: Vec<Param>,
    /// Generated query-parameter struct, when the operation declares query
    /// parameters. Its name doubles as the `axum_extra::extract::Query<..>`
    /// type and the `Api` method's `query` argument type.
    pub query: Option<Struct>,
    /// Generated header-parameter struct, when the operation declares header
    /// parameters. Its name doubles as the generated `FromRequestParts`
    /// extractor type and the `Api` method's `headers` argument type.
    pub headers: Option<Headers>,
    /// Generated cookie-parameter struct, when the operation declares cookie
    /// parameters. Its name doubles as the generated `FromRequestParts`
    /// extractor type and the `Api` method's `cookies` argument type.
    pub cookies: Option<Cookies>,
    /// JSON request body type, when the operation declares one.
    pub body: Option<Body>,
    /// Response variants, in declaration order.
    pub responses: Vec<ResponseCase>,
}

/// A typed operation parameter (path parameter in the current slice).
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Rust argument identifier.
    pub name: RustIdent,
    /// Parameter type.
    pub ty: RustType,
}

/// A generated per-operation header struct, extracted via a hand-written
/// `axum::extract::FromRequestParts` implementation rather than serde, since
/// header values are read and parsed individually from the request parts.
#[derive(Debug, Clone, PartialEq)]
pub struct Headers {
    /// Struct name (`<Op>Headers`), doubling as the extractor type and the
    /// `Api` method's `headers` argument type.
    pub name: RustIdent,
    /// Header fields, in declaration order.
    pub params: Vec<HeaderParam>,
}

/// A single header parameter within a [`Headers`] struct.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderParam {
    /// Rust field identifier (`snake_case`).
    pub name: RustIdent,
    /// The exact OpenAPI header name, used for the case-insensitive lookup in
    /// the generated extractor (e.g. `X-Request-Id`).
    pub header_name: String,
    /// The parsed scalar type. Unlike [`Field`], this is the bare element type
    /// even when the header is optional; the emitter adds the `Option<..>`
    /// wrapper for absent headers.
    pub ty: RustType,
    /// Whether the header is required. A missing required header is a `400`.
    pub required: bool,
    /// Doc comment derived from the parameter `description`.
    pub doc: Option<String>,
}

/// A generated per-operation cookie struct, extracted via a hand-written
/// `axum::extract::FromRequestParts` implementation backed by `axum_extra`'s
/// `CookieJar`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cookies {
    /// Struct name (`<Op>Cookies`), doubling as the extractor type and the
    /// `Api` method's `cookies` argument type.
    pub name: RustIdent,
    /// Cookie fields, in declaration order.
    pub params: Vec<CookieParam>,
}

/// A single cookie parameter within a [`Cookies`] struct.
#[derive(Debug, Clone, PartialEq)]
pub struct CookieParam {
    /// Rust field identifier (`snake_case`).
    pub name: RustIdent,
    /// The exact OpenAPI cookie name, used for the `CookieJar` lookup.
    pub cookie_name: String,
    /// The parsed scalar type. Like [`HeaderParam`], this is the bare element
    /// type even when optional; the emitter adds the `Option<..>` wrapper.
    pub ty: RustType,
    /// Whether the cookie is required. A missing required cookie is a `400`.
    pub required: bool,
    /// Doc comment derived from the parameter `description`.
    pub doc: Option<String>,
}

/// One arm of an operation's response enum.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseCase {
    /// Variant identifier, named after the status reason phrase (fixed codes),
    /// the response-range class, or `Default`.
    pub variant: RustIdent,
    /// How the variant's HTTP status code is determined.
    pub status: ResponseStatus,
    /// JSON response body type, when the response declares content.
    pub body: Option<Body>,
    /// Declared response headers written by the generated `IntoResponse`, in
    /// declaration order. Empty means no headers (the pre-C5 variant shape).
    pub headers: Vec<ResponseHeader>,
    /// Doc comment derived from the response `description`.
    pub doc: Option<String>,
}

/// A single declared response header written by the generated `IntoResponse`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseHeader {
    /// Rust field identifier (`snake_case`).
    pub name: RustIdent,
    /// Exact header name as written to the response (e.g. `X-Request-Id`).
    pub header_name: String,
    /// Scalar type serialized to a header value via `ToString`. Bare element
    /// type even when optional; the emitter adds the `Option<..>` wrapper.
    pub ty: RustType,
    /// Whether the header is always written (`false` → `Option<..>` field).
    pub required: bool,
    /// Doc comment derived from the header `description`.
    pub doc: Option<String>,
}

/// How a response variant's HTTP status code is produced.
///
/// Fixed codes are emitted as a compile-time constant; `default` and range
/// responses have no single code, so the variant instead carries an
/// `axum::http::StatusCode` the handler supplies at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseStatus {
    /// A concrete status code (e.g. `200`), emitted as a `StatusCode` constant.
    Fixed(u16),
    /// The `default` catch-all response; the handler supplies the status code.
    Default,
    /// A status-code range such as `5XX`, carrying the leading digit (`1..=5`);
    /// the handler supplies a concrete code within the class.
    Range(u8),
}
