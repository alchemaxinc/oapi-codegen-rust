//! Intermediate representation (IR) of generated Rust types.
//!
//! The schema mapping pass ([`crate::lower::schema`]) lowers OpenAPI schemas into this
//! IR. The emit pass ([`crate::emit`]) turns the IR into a token stream. Keeping
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
    /// `#[deprecated]` annotation from `deprecated: true` (+ `x-deprecated-reason`).
    pub deprecated: Option<Deprecation>,
    /// Named fields, in deterministic order.
    pub fields: Vec<Field>,
    /// When set, the struct captures unknown keys into a flattened map of this
    /// element type (`additionalProperties`).
    pub additional_properties: Option<RustType>,
    /// Whether the schema set `additionalProperties: false`, which becomes
    /// `#[serde(deny_unknown_fields)]`.
    ///
    /// This is a field of its own and not the `None` case of
    /// [`Self::additional_properties`], because an absent `additionalProperties`
    /// and an explicit `false` are different statements. An absent key permits
    /// unknown keys and drops them, which is what serde does with no attribute.
    /// An explicit `false` rejects them. Both give no flattened map, so the map
    /// alone cannot tell them apart.
    pub deny_unknown_fields: bool,
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
    /// `#[deprecated]` annotation from `deprecated: true` (+ `x-deprecated-reason`).
    pub deprecated: Option<Deprecation>,
    /// Field type (already wrapped in `Option<..>` when optional).
    pub ty: RustType,
    /// Whether the property is required. Optional fields get
    /// `#[serde(skip_serializing_if = "Option::is_none")]` unless [`Self::omit_empty`]
    /// overrides it.
    pub required: bool,
    /// `x-omitempty` override: `Some(true)`/`Some(false)` forces the
    /// `skip_serializing_if` on/off. `None` keeps the default (skip when optional).
    pub omit_empty: Option<bool>,
    /// `x-rust-serde-skip`: drop the field from (de)serialization via `#[serde(skip)]`.
    pub serde_skip: bool,
    /// The value that serde uses when the property is absent, from `default`.
    ///
    /// An optional property with a default is *not* wrapped in `Option`. Once
    /// parsed, it always holds a value. Only `nullable` keeps the `Option`,
    /// because there `null` is a value that the property can carry.
    ///
    /// `default: null` never reaches here. The parser reads it as no default at
    /// all, and serde already leaves a missing `Option` as `None`.
    pub default: Option<DefaultValue>,
    /// The validation keywords the property declares, when it declares any.
    ///
    /// The generator checks these on the way in, so the check runs only where
    /// the code deserializes. A response the server writes is not checked.
    pub constraints: Option<Constraints>,
}

/// A numeric bound, in the form the document writes it.
///
/// A bound holds one number, so it copies. `f64` has no `Eq`, so the list stops
/// at `PartialEq`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bound {
    /// A bound on an `integer` schema.
    Int(i64),
    /// A bound on a `number` schema.
    Float(f64),
}

/// The validation keywords a schema declares.
///
/// A field with no keyword carries `None`, so the common schema costs nothing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Constraints {
    /// `pattern`: the value must match this regular expression.
    pub pattern: Option<String>,
    /// `minLength`, counted in characters, as JSON Schema counts them.
    pub min_length: Option<usize>,
    /// `maxLength`, counted in characters.
    pub max_length: Option<usize>,
    /// `minimum`.
    pub minimum: Option<Bound>,
    /// `maximum`.
    pub maximum: Option<Bound>,
    /// `exclusiveMinimum`: makes `minimum` a strict bound.
    pub exclusive_minimum: bool,
    /// `exclusiveMaximum`: makes `maximum` a strict bound.
    pub exclusive_maximum: bool,
    /// `multipleOf`.
    pub multiple_of: Option<Bound>,
    /// `minItems`.
    pub min_items: Option<usize>,
    /// `maxItems`.
    pub max_items: Option<usize>,
    /// `uniqueItems`.
    pub unique_items: bool,
    /// `minProperties`.
    pub min_properties: Option<usize>,
    /// `maxProperties`.
    pub max_properties: Option<usize>,
    /// The type the checks run against, when the field names an alias.
    ///
    /// A `$ref` to a constrained scalar gives the field a named type, and the
    /// name alone does not say which check applies. Lowering resolves the ref
    /// and records the type behind the name here.
    pub checked_as: Option<RustType>,
}

impl Constraints {
    /// Whether the schema declares no keyword at all.
    pub fn is_empty(&self) -> bool {
        return *self == Self::default();
    }
}

/// The value that serde uses when a property is absent.
///
/// Lowered from the schema `default` and already checked against the field type.
/// The JSON value alone is not enough to write Rust. `1` is `1` for an integer
/// field and `1.0` for a number field. `"active"` is a string for a `String`
/// field and a variant path for an enum.
#[derive(Debug, Clone, PartialEq)]
pub enum DefaultValue {
    /// A string literal.
    Str(String),
    /// An integer literal.
    Int(i64),
    /// A floating-point literal.
    Float(f64),
    /// A `bool` literal.
    Bool(bool),
    /// A unit variant of a generated string enum, named by its identifier.
    Variant(RustIdent),
    /// An empty collection, which `Default::default()` gives for both `Vec` and
    /// `HashMap`.
    Empty,
}

/// A generated `enum`.
#[derive(Debug, Clone, PartialEq)]
pub struct Enum {
    /// Type name.
    pub name: RustIdent,
    /// Doc comment derived from the schema `description`.
    pub doc: Option<String>,
    /// `#[deprecated]` annotation from `deprecated: true` (+ `x-deprecated-reason`).
    pub deprecated: Option<Deprecation>,
    /// The flavour of enum to emit.
    pub kind: EnumKind,
}

/// The flavour of a generated enum.
#[derive(Debug, Clone, PartialEq)]
pub enum EnumKind {
    /// A C-like string enum: unit variants mapped to wire strings.
    Strings(Vec<StringVariant>),
    /// A C-like integer enum: unit variants with explicit discriminants.
    ///
    /// The wire form is a bare number, so the emitted type carries
    /// `#[serde(try_from, into)]` and a `#[repr]` of [`Self::Integers::repr`].
    Integers {
        /// The integer type the discriminants take, either `i32` or `i64`.
        repr: RustType,
        /// The permitted values, in document order.
        variants: Vec<IntegerVariant>,
    },
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

/// A unit variant of an integer enum.
#[derive(Debug, Clone, PartialEq)]
pub struct IntegerVariant {
    /// Rust variant identifier.
    pub name: RustIdent,
    /// The discriminant, and the value on the wire.
    pub value: i64,
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
    /// `#[deprecated]` annotation from `deprecated: true` (+ `x-deprecated-reason`).
    pub deprecated: Option<Deprecation>,
    /// Aliased type.
    pub ty: RustType,
}

/// A `#[deprecated]` annotation derived from `deprecated: true`, optionally
/// carrying the `x-deprecated-reason` text as the `note`.
#[derive(Debug, Clone, PartialEq)]
pub struct Deprecation {
    /// The `x-deprecated-reason` text, emitted as `#[deprecated(note = "...")]`.
    pub note: Option<String>,
}

/// Which of the three non-serde traits every generated model derives a foreign
/// type satisfies.
///
/// The generator derives `Debug`, `Clone`, and `PartialEq` on every model. A
/// model that holds a type the generator did not write cannot derive a trait that
/// type lacks. The generator also cannot inspect the type, because the
/// specification names it as text and `rustc` resolves it much later. So the
/// specification author declares it, with `x-rust-derive`.
///
/// [`Self::default`] claims all three. That keeps the output of every
/// specification written before this feature identical, and it is right far more
/// often than not. `uuid::Uuid`, the `chrono` types, and a typical hand-written
/// domain type all derive the three. A target that does not is the case worth one
/// line of specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignDerives {
    /// The target implements [`std::fmt::Debug`].
    pub debug: bool,
    /// The target implements [`Clone`].
    pub clone: bool,
    /// The target implements [`PartialEq`].
    pub partial_eq: bool,
}

impl Default for ForeignDerives {
    fn default() -> Self {
        return Self {
            debug: true,
            clone: true,
            partial_eq: true,
        };
    }
}

impl ForeignDerives {
    /// Whether this target satisfies all three traits, and therefore constrains
    /// nothing. The common case, and the one that needs no record anywhere.
    pub(crate) fn is_unconstrained(self) -> bool {
        return self.debug && self.clone && self.partial_eq;
    }

    /// Narrow to the traits both `self` and `other` satisfy.
    ///
    /// A model holding two foreign types can derive only what both allow, so the
    /// walk over a model's referenced types folds with this.
    pub(crate) fn intersect(self, other: Self) -> Self {
        return Self {
            debug: self.debug && other.debug,
            clone: self.clone && other.clone,
            partial_eq: self.partial_eq && other.partial_eq,
        };
    }
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
    /// `Box<T>`, added by the recursion pass to give a cyclic type a size.
    ///
    /// Nothing in a schema asks for this. `lower::recurse` inserts it where a
    /// type would otherwise hold itself, directly or through other types.
    Boxed(Box<RustType>),
    /// A reference to a named (generated or external) type.
    Named(String),
    /// A reference to a type from an import-mapped module: `module::Name`.
    ///
    /// Generated by another run of this generator, so it derives all three
    /// non-serde traits and carries no [`ForeignDerives`].
    External { module: String, name: String },
    /// A verbatim type expression from an `x-rust-type` extension, with what its
    /// `x-rust-derive` says the target implements.
    Verbatim {
        /// The Rust type expression, emitted as written.
        text: String,
        /// Which of `Debug`, `Clone`, `PartialEq` the target satisfies.
        derives: ForeignDerives,
    },
}

impl RustType {
    /// Wrap this type in `Option<..>`.
    pub fn optional(self) -> RustType {
        return RustType::Option(Box::new(self));
    }

    /// Whether this type is an `Option<..>` (for which `skip_serializing_if =
    /// "Option::is_none"` is valid).
    pub fn is_option(&self) -> bool {
        return matches!(self, RustType::Option(_));
    }

    /// The type inside any `Option` or `Box` wrapper.
    ///
    /// A `Vec` and a `Map` are not wrappers here. Each carries keywords of its
    /// own, so a rule reads the collection and not an element.
    pub fn innermost(&self) -> &RustType {
        return match self {
            RustType::Option(inner) | RustType::Boxed(inner) => inner.innermost(),
            other => other,
        };
    }

    /// Whether this is a scalar the generated code compares with `==`.
    pub fn is_scalar(&self) -> bool {
        return matches!(
            self,
            RustType::Bool | RustType::I32 | RustType::I64 | RustType::F64 | RustType::String
        );
    }

    /// The type as text, for an error message.
    ///
    /// Emission goes through `emit::emit_type`, which builds tokens. This gives
    /// a reader the same type in a message that the lowering stage can write,
    /// where no token stream exists yet.
    pub fn label(&self) -> String {
        return match self {
            RustType::Bool => "bool".to_owned(),
            RustType::I32 => "i32".to_owned(),
            RustType::I64 => "i64".to_owned(),
            RustType::F64 => "f64".to_owned(),
            RustType::String => "String".to_owned(),
            RustType::Value => "serde_json::Value".to_owned(),
            RustType::Date => "chrono::NaiveDate".to_owned(),
            RustType::DateTime => "chrono::DateTime<chrono::Utc>".to_owned(),
            RustType::Uuid => "uuid::Uuid".to_owned(),
            RustType::Bytes => "Vec<u8>".to_owned(),
            RustType::Vec(inner) => format!("Vec<{}>", inner.label()),
            RustType::Map(inner) => format!("std::collections::HashMap<String, {}>", inner.label()),
            RustType::Option(inner) => format!("Option<{}>", inner.label()),
            RustType::Boxed(inner) => format!("Box<{}>", inner.label()),
            RustType::Named(name) => name.clone(),
            RustType::External { module, name } => format!("{module}::{name}"),
            RustType::Verbatim { text, .. } => text.clone(),
        };
    }

    /// An `x-rust-type` target whose `x-rust-derive` is absent, so it claims all
    /// three non-serde traits. The common case, and the shape every test that
    /// does not test this feature wants.
    pub fn verbatim(text: impl Into<String>) -> RustType {
        return RustType::Verbatim {
            text: text.into(),
            derives: ForeignDerives::default(),
        };
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
    /// `multipart/form-data` (request bodies only) → a hand-written
    /// `FromRequest` extractor driving `axum::extract::Multipart`. Carried
    /// alongside an [`Operation`]'s [`Multipart`], which holds the per-field
    /// parsing detail the extractor needs.
    Multipart,
}

/// A `multipart/form-data` request body lowered into a per-operation extractor.
///
/// axum has no *typed* multipart extractor, so the generator emits a dedicated
/// struct (an operation artifact, like the query/header/cookie structs) plus a
/// hand-written `FromRequest` implementation that drives
/// `axum::extract::Multipart`, reads each declared field, and returns a
/// `400 Bad Request` on a missing required field or an unparseable value.
#[derive(Debug, Clone, PartialEq)]
pub struct Multipart {
    /// Struct name (`<Op>Multipart`), doubling as the generated `FromRequest`
    /// extractor type and the `Api` method's `body` argument type.
    pub name: RustIdent,
    /// Fields parsed from the multipart stream, in declaration order.
    pub fields: Vec<MultipartField>,
}

/// A single field of a [`Multipart`] body.
#[derive(Debug, Clone, PartialEq)]
pub struct MultipartField {
    /// Wire field name (the part's `Content-Disposition` `name`).
    pub wire_name: String,
    /// Target struct field identifier on the generated `<Op>Multipart` struct.
    pub rust_name: RustIdent,
    /// The decoded field type: `Vec<u8>` for a binary (file) part, else a
    /// scalar. This is the bare inner type even when the field is optional.
    /// [`MultipartField::optional`] records whether the struct wraps it in
    /// `Option<..>`.
    pub ty: RustType,
    /// Whether the generated struct wraps this field in `Option<..>` (true when
    /// the property is not `required`, or is `nullable`). An absent
    /// non-optional field is a `400`. An absent optional field is `None`.
    pub optional: bool,
    /// Whether the part is a binary/file field read as raw bytes (`Vec<u8>`).
    pub is_file: bool,
}

/// An operation's request payload, when it declares one.
///
/// The three cases are mutually exclusive by construction, replacing what will
/// otherwise be several mutually-exclusive `Option` fields on [`Operation`].
#[derive(Debug, Clone, PartialEq)]
pub enum RequestPayload {
    /// A single supported content type (JSON, `text/plain`, or form), extracted
    /// directly by the matching axum extractor.
    Single(Body),
    /// A `multipart/form-data` body, decoded by a generated [`Multipart`]
    /// extractor.
    Multipart(Multipart),
    /// Several supported content types, dispatched at request time on the
    /// incoming `Content-Type` header by a generated [`NegotiatedBody`]
    /// `FromRequest` enum. An unrecognised or missing content type yields a
    /// `415 Unsupported Media Type`.
    Negotiated(NegotiatedBody),
}

/// A body offering several content-type representations, lowered into a
/// generated enum with one variant per representation.
///
/// For a request the enum is a hand-written `FromRequest` that dispatches on
/// `Content-Type`. for a response it is a plain enum the handler selects a
/// representation from, which the generated `IntoResponse` renders with the
/// matching `Content-Type`.
#[derive(Debug, Clone, PartialEq)]
pub struct NegotiatedBody {
    /// Enum name — `<Op>RequestBody` for a request, `<Response><Variant>Body`
    /// for a response — doubling as the argument/field type on the generated
    /// interface.
    pub name: RustIdent,
    /// One variant per supported content type, in priority order (JSON > form >
    /// text).
    pub variants: Vec<BodyVariant>,
}

/// One content-type representation within a [`NegotiatedBody`].
#[derive(Debug, Clone, PartialEq)]
pub struct BodyVariant {
    /// Variant identifier, named after the content kind (`Json`, `Form`,
    /// `Text`).
    pub variant: RustIdent,
    /// The decoded body type and content kind for this representation.
    pub body: Body,
}

/// The body of a response variant, when it declares supported content.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponseBody {
    /// A single supported content type, rendered by the matching axum response
    /// wrapper.
    Single(Body),
    /// Several supported content types the handler chooses among. The generated
    /// `IntoResponse` renders whichever representation the handler selected.
    Negotiated(NegotiatedBody),
}

/// A generated axum server interface: the `Api` trait plus the operations that
/// back its `Router`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Service {
    /// Operations in deterministic (document) order.
    pub operations: Vec<Operation>,
    /// Security schemes referenced by at least one operation, in the order they
    /// are declared in `components.securitySchemes`. The client emitter turns
    /// each into a credential field and a `with_<scheme>` builder setter. The
    /// server emitter ignores them (server-side auth is not generated yet).
    pub security_schemes: Vec<SecurityScheme>,
}

/// A security scheme the client can apply to outgoing requests.
///
/// Derived from a `components.securitySchemes` entry. only the schemes the
/// client generator can carry as a stored credential are modelled here, plus an
/// [`SecuritySchemeKind::Unsupported`] catch-all so an operation that *requires*
/// an unmodelled scheme (for example OAuth2) can be rejected with a clear message.
#[derive(Debug, Clone, PartialEq)]
pub struct SecurityScheme {
    /// The scheme's key in `components.securitySchemes`, matched against each
    /// [`Operation::security`] entry.
    pub key: String,
    /// `snake_case` base name for the generated credential field and the
    /// `with_<field>` builder setter.
    pub field: RustIdent,
    /// How the credential is carried on the request.
    pub kind: SecuritySchemeKind,
    /// Doc comment derived from the scheme's `description`.
    pub doc: Option<String>,
}

/// How a [`SecurityScheme`]'s credential is applied to a request.
#[derive(Debug, Clone, PartialEq)]
pub enum SecuritySchemeKind {
    /// `type: http, scheme: bearer` — `Authorization: Bearer <token>`.
    HttpBearer,
    /// `type: http, scheme: basic` — `Authorization: Basic <base64>`.
    HttpBasic,
    /// `type: apiKey, in: header` — the key is sent as the named header.
    ApiKeyHeader(String),
    /// `type: apiKey, in: query` — the key is sent as the named query parameter.
    ApiKeyQuery(String),
    /// `type: apiKey, in: cookie` — the key is sent as the named cookie.
    ApiKeyCookie(String),
    /// A scheme the client cannot carry as a stored credential (`oauth2`,
    /// `openIdConnect`). The wrapped string is a human-readable reason used when
    /// rejecting an operation that requires it.
    Unsupported(String),
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
    /// Request payload (a single supported content type, a `multipart/form-data`
    /// extractor, or a `Content-Type`-dispatched set of content types), when the
    /// operation declares a request body.
    pub request: Option<RequestPayload>,
    /// Response variants, in declaration order.
    pub responses: Vec<ResponseCase>,
    /// Keys of the security schemes this operation applies, derived from its
    /// effective security requirement (its own `security`, else the document's
    /// global `security`). Each key matches a [`SecurityScheme::key`]. Empty
    /// means the operation is unauthenticated.
    pub security: Vec<String>,
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
    /// the generated extractor (for example `X-Request-Id`).
    pub header_name: String,
    /// The parsed scalar type. Unlike [`Field`], this is the bare element type
    /// even when the header is optional. The emitter adds the `Option<..>`
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
    /// type even when optional. The emitter adds the `Option<..>` wrapper.
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
    /// Response body (a single supported content type, or a set of content types
    /// the handler chooses among), when the response declares supported content.
    pub body: Option<ResponseBody>,
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
    /// Exact header name as written to the response (for example `X-Request-Id`).
    pub header_name: String,
    /// Scalar type serialized to a header value via `ToString`. Bare element
    /// type even when optional. The emitter adds the `Option<..>` wrapper.
    pub ty: RustType,
    /// Whether the header is always written (`false` → `Option<..>` field).
    pub required: bool,
    /// Doc comment derived from the header `description`.
    pub doc: Option<String>,
}

/// How a response variant's HTTP status code is produced.
///
/// Fixed codes are emitted as a compile-time constant. `default` and range
/// responses have no single code, so the variant instead carries an
/// `axum::http::StatusCode` the handler supplies at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseStatus {
    /// A concrete status code (for example `200`), emitted as a `StatusCode` constant.
    Fixed(u16),
    /// The `default` catch-all response. The handler supplies the status code.
    Default,
    /// A status-code range such as `5XX`, carrying the leading digit (`1..=5`).
    /// the handler supplies a concrete code within the class.
    Range(u8),
}

/// The lowered `servers:` block: constants and builder functions for each
/// declared server URL, plus the enum types their variables reference.
///
/// Emitted when `generate.server-urls` is set. A server whose URL has no
/// `{placeholder}` becomes a `const`. one with placeholders becomes a builder
/// function that substitutes each variable and validates the result.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerUrls {
    /// Enum types for the enum-constrained server variables, emitted before the
    /// builder functions that reference them.
    pub enums: Vec<ServerUrlEnum>,
    /// One entry per declared server, in document order.
    pub servers: Vec<ServerUrl>,
}

/// A single lowered server URL: either a constant or a builder function.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerUrl {
    /// A server URL with no variables: `pub const <NAME>: &str = "<url>";`.
    Const(ServerUrlConst),
    /// A server URL with `{placeholder}`s: a builder function substituting them.
    Builder(ServerUrlBuilder),
}

/// A variable-free server URL emitted as a string constant.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerUrlConst {
    /// `SCREAMING_SNAKE_CASE` constant name.
    pub name: RustIdent,
    /// Doc comment derived from the server `description`.
    pub doc: Option<String>,
    /// The literal server URL.
    pub url: String,
}

/// A server URL with `{placeholder}`s emitted as a builder function that
/// substitutes each variable and returns the resulting URL.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerUrlBuilder {
    /// `snake_case` function name.
    pub name: RustIdent,
    /// Doc comment derived from the server `description`.
    pub doc: Option<String>,
    /// The URL template, retaining its `{placeholder}` tokens.
    pub url_template: String,
    /// Parameters, sorted by placeholder name for a deterministic signature.
    pub params: Vec<ServerUrlParam>,
}

/// One parameter of a [`ServerUrlBuilder`], bound to a URL placeholder.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerUrlParam {
    /// `snake_case` parameter identifier.
    pub ident: RustIdent,
    /// The placeholder name as it appears in the URL (without braces).
    pub placeholder: String,
    /// How the parameter is typed and turned into its substituted string.
    pub ty: ServerUrlParamType,
}

/// The type of a [`ServerUrlParam`].
#[derive(Debug, Clone, PartialEq)]
pub enum ServerUrlParamType {
    /// A free-form `&str` parameter (a non-enum or undeclared variable).
    Str,
    /// An enum-constrained parameter of the named [`ServerUrlEnum`] type.
    Enum(RustIdent),
}

/// An enum type generated for an enum-constrained server variable.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerUrlEnum {
    /// `PascalCase` enum type name (`<Server><Variable>`).
    pub name: RustIdent,
    /// Doc comment naming the server variable this enum constrains.
    pub doc: Option<String>,
    /// The permitted values, in declaration order.
    pub variants: Vec<ServerUrlEnumVariant>,
    /// The variant identifier used for the `Default` impl (the OpenAPI
    /// `default`), when the variable declares one.
    pub default: Option<RustIdent>,
}

/// One variant of a [`ServerUrlEnum`]: a Rust identifier and its wire value.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerUrlEnumVariant {
    /// `PascalCase` variant identifier.
    pub name: RustIdent,
    /// The wire value substituted into the URL.
    pub value: String,
}
