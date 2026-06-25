//! Intermediate representation (IR) of generated Rust types.
//!
//! The schema mapping pass ([`crate::schema`]) lowers OpenAPI schemas into this
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
    /// Internal handler function name (`<name>_handler`).
    pub handler: RustIdent,
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
    /// JSON request body type, when the operation declares one.
    pub body: Option<RustType>,
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

/// One arm of an operation's response enum.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseCase {
    /// Variant identifier, named after the status reason phrase.
    pub variant: RustIdent,
    /// Numeric HTTP status code, emitted as `StatusCode::from_u16(..)`.
    pub status: u16,
    /// JSON response body type, when the response declares content.
    pub body: Option<RustType>,
    /// Doc comment derived from the response `description`.
    pub doc: Option<String>,
}
