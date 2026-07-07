//! Single source of truth for OpenAPI 3.0 schema-feature coverage.
//!
//! For every OpenAPI 3.0 construct the models generator must do exactly one of:
//! generate Rust ([`Status::Supported`]), reject it with a documented error
//! ([`Status::Unsupported`]), recognise but intentionally drop it
//! ([`Status::Ignored`]), or defer it to server/client generation
//! ([`Status::Planned`]). Nothing may be left in the unknown.
//!
//! Two mechanisms enforce this:
//!  1. [`TEST_TABLE`] catalogues every feature and links the testable ones to a
//!     minimal fixture under `tests/fixtures/`. The tests below assert that each
//!     supported fixture matches its generated output, each unsupported fixture
//!     is rejected, and no fixture is left uncatalogued.
//!  2. The [`anchor`] module matches every `openapiv3` schema enum without a
//!     wildcard, so adding a variant upstream fails to compile until it is
//!     handled and catalogued here.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

/// How the generator treats an OpenAPI construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// Generated to Rust; exercised by a fixture and generated file.
    Supported,
    /// Deliberately rejected with an error; exercised by a fixture.
    Unsupported,
    /// Parsed but intentionally not reflected in the output.
    Ignored,
    /// Belongs to server/client generation, not implemented yet.
    Planned,
}

/// One row of the coverage test table.
struct Feature {
    /// Dotted OpenAPI element path, e.g. `schema.oneOf`.
    element: &'static str,
    /// How the generator handles it.
    status: Status,
    /// Fixture stem under `tests/fixtures` (and generated output under
    /// `tests/generated`) that exercises this feature, when one applies.
    fixture: Option<&'static str>,
}

/// The complete catalogue of OpenAPI 3.0 schema features and their handling.
const TEST_TABLE: &[Feature] = &[
    // Schema kinds
    Feature {
        element: "schema.type.string",
        status: Status::Supported,
        fixture: Some("primitive_scalars"),
    },
    Feature {
        element: "schema.type.integer",
        status: Status::Supported,
        fixture: Some("primitive_scalars"),
    },
    Feature {
        element: "schema.type.number",
        status: Status::Supported,
        fixture: Some("primitive_scalars"),
    },
    Feature {
        element: "schema.type.boolean",
        status: Status::Supported,
        fixture: Some("primitive_scalars"),
    },
    Feature {
        element: "schema.type.object",
        status: Status::Supported,
        fixture: Some("object_optional_required"),
    },
    Feature {
        element: "schema.type.array",
        status: Status::Supported,
        fixture: Some("array_types"),
    },
    Feature {
        element: "schema.type.string.enum",
        status: Status::Supported,
        fixture: Some("string_enum"),
    },
    Feature {
        element: "schema.oneOf",
        status: Status::Supported,
        fixture: Some("oneof_untagged"),
    },
    Feature {
        element: "schema.oneOf.discriminator",
        status: Status::Supported,
        fixture: Some("oneof_discriminator"),
    },
    Feature {
        element: "schema.anyOf",
        status: Status::Supported,
        fixture: Some("anyof_untagged"),
    },
    Feature {
        element: "schema.allOf",
        status: Status::Supported,
        fixture: Some("allof_merge"),
    },
    Feature {
        element: "schema.any",
        status: Status::Supported,
        fixture: Some("freeform_any"),
    },
    Feature {
        element: "schema.not",
        status: Status::Unsupported,
        fixture: Some("unsupported_not"),
    },
    // String formats
    Feature {
        element: "format.string.none",
        status: Status::Supported,
        fixture: Some("string_formats"),
    },
    Feature {
        element: "format.string.date",
        status: Status::Supported,
        fixture: Some("string_formats"),
    },
    Feature {
        element: "format.string.date-time",
        status: Status::Supported,
        fixture: Some("string_formats"),
    },
    Feature {
        element: "format.string.byte",
        status: Status::Supported,
        fixture: Some("string_formats"),
    },
    Feature {
        element: "format.string.binary",
        status: Status::Supported,
        fixture: Some("string_formats"),
    },
    Feature {
        element: "format.string.password",
        status: Status::Supported,
        fixture: Some("string_formats"),
    },
    Feature {
        element: "format.string.uuid",
        status: Status::Supported,
        fixture: Some("string_formats"),
    },
    Feature {
        element: "format.string.unknown",
        status: Status::Supported,
        fixture: Some("string_formats"),
    },
    // Integer formats
    Feature {
        element: "format.integer.none",
        status: Status::Supported,
        fixture: Some("integer_formats"),
    },
    Feature {
        element: "format.integer.int32",
        status: Status::Supported,
        fixture: Some("integer_formats"),
    },
    Feature {
        element: "format.integer.int64",
        status: Status::Supported,
        fixture: Some("integer_formats"),
    },
    // Number formats
    Feature {
        element: "format.number.none",
        status: Status::Supported,
        fixture: Some("number_formats"),
    },
    Feature {
        element: "format.number.float",
        status: Status::Supported,
        fixture: Some("number_formats"),
    },
    Feature {
        element: "format.number.double",
        status: Status::Supported,
        fixture: Some("number_formats"),
    },
    // Object features
    Feature {
        element: "object.required",
        status: Status::Supported,
        fixture: Some("object_optional_required"),
    },
    Feature {
        element: "object.optional",
        status: Status::Supported,
        fixture: Some("object_optional_required"),
    },
    Feature {
        element: "object.properties.inlineObject",
        status: Status::Supported,
        fixture: Some("object_nested_inline"),
    },
    Feature {
        element: "object.additionalProperties.schema",
        status: Status::Supported,
        fixture: Some("object_additional_properties"),
    },
    Feature {
        element: "object.additionalProperties.true",
        status: Status::Supported,
        fixture: Some("map_alias"),
    },
    Feature {
        element: "object.additionalProperties.false",
        status: Status::Ignored,
        fixture: None,
    },
    // References
    Feature {
        element: "schema.$ref.local",
        status: Status::Supported,
        fixture: Some("ref_local"),
    },
    // Schema metadata
    Feature {
        element: "meta.description",
        status: Status::Supported,
        fixture: Some("metadata_docs"),
    },
    Feature {
        element: "meta.nullable",
        status: Status::Supported,
        fixture: Some("nullable"),
    },
    Feature {
        element: "meta.title",
        status: Status::Ignored,
        fixture: None,
    },
    Feature {
        element: "meta.default",
        status: Status::Ignored,
        fixture: None,
    },
    Feature {
        element: "meta.deprecated",
        status: Status::Ignored,
        fixture: None,
    },
    Feature {
        element: "meta.readOnly",
        status: Status::Ignored,
        fixture: None,
    },
    Feature {
        element: "meta.writeOnly",
        status: Status::Ignored,
        fixture: None,
    },
    Feature {
        element: "meta.example",
        status: Status::Ignored,
        fixture: None,
    },
    Feature {
        element: "meta.externalDocs",
        status: Status::Ignored,
        fixture: None,
    },
    // Extensions
    Feature {
        element: "ext.x-rust-type",
        status: Status::Supported,
        fixture: Some("ext_x_rust_type"),
    },
    Feature {
        element: "ext.x-go-*",
        status: Status::Ignored,
        fixture: None,
    },
    // Document-level (server/client generation). The axum server generator now
    // covers a slice of paths/parameters/requestBody/responses; that slice is
    // validated separately by `SERVER_FIXTURES`. The rest remains deferred.
    Feature {
        element: "doc.paths",
        status: Status::Planned,
        fixture: None,
    },
    Feature {
        element: "doc.parameters",
        status: Status::Planned,
        fixture: None,
    },
    Feature {
        element: "doc.requestBody",
        status: Status::Planned,
        fixture: None,
    },
    Feature {
        element: "doc.responses",
        status: Status::Planned,
        fixture: None,
    },
    Feature {
        element: "doc.components.securitySchemes",
        status: Status::Planned,
        fixture: None,
    },
    Feature {
        element: "doc.servers",
        status: Status::Planned,
        fixture: None,
    },
    Feature {
        element: "doc.callbacks",
        status: Status::Planned,
        fixture: None,
    },
    Feature {
        element: "doc.links",
        status: Status::Planned,
        fixture: None,
    },
];

/// Server fixtures whose generated axum interface is compile-checked against a
/// committed file.
///
/// These exercise the server generator (a different axis from the schema
/// [`TEST_TABLE`]): path parameters, JSON request bodies, and typed responses.
/// Each must have a `#[test]` via [`server_generated_tests!`].
const SERVER_FIXTURES: &[&str] = &[
    "server_petstore",
    "server_refs",
    "server_query_params",
    "server_header_params",
    "server_default_range_responses",
    "server_cookie_params",
    "server_component_param_ref",
    "server_component_body_ref",
    "server_component_param_ref_pet",
    "server_xfile_refs",
    "server_response_headers",
];

/// Server fixtures whose generation must fail with a documented error, covering
/// the slice's deliberate limitations (e.g. an unrecognised HTTP status code).
const SERVER_UNSUPPORTED_FIXTURES: &[&str] = &[
    "server_unsupported_unknown_status",
    "server_unsupported_xfile_response_ref",
    "server_unsupported_object_path_param",
    "server_unsupported_object_query_param",
    "server_unsupported_explode_false_query_param",
    "server_unsupported_array_header_param",
    "server_unsupported_object_cookie_param",
    "server_unsupported_bytes_cookie_param",
    "server_unsupported_xfile_param_ref",
    "server_unsupported_xfile_body_ref",
    "server_unsupported_xfile_missing_component",
    "server_unsupported_xfile_no_import_mapping",
    "server_unsupported_xfile_object_path_param",
    "server_unsupported_object_response_header",
    "server_unsupported_bytes_response_header",
];

/// Absolute path to the crate's `tests` directory.
fn tests_dir() -> PathBuf {
    return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
}

/// Distinct fixture stems referenced by rows with the given status.
fn fixtures_with(status: Status) -> BTreeSet<&'static str> {
    return TEST_TABLE
        .iter()
        .filter(|feature| {
            return feature.status == status;
        })
        .filter_map(|feature| {
            return feature.fixture;
        })
        .collect();
}

/// Regenerate `stem`'s output and assert it matches the committed file.
///
/// Refresh the committed files after an intentional change with
/// `make update-generated`
/// (`UPDATE_GENERATED=1 cargo test -p oapi-codegen --test coverage`).
fn assert_generated_matches(stem: &str) {
    let dir = tests_dir();
    let fixture = dir.join("fixtures").join(format!("{stem}.yaml"));
    let generated_file = dir.join("generated").join(format!("{stem}.rs"));

    let generated = oapi_codegen::generate_models_string(&fixture).unwrap_or_else(|err| {
        panic!("generating `{stem}` failed: {err}");
    });

    if std::env::var_os("UPDATE_GENERATED").is_some() {
        std::fs::write(&generated_file, &generated).unwrap_or_else(|err| {
            panic!("writing `{stem}` failed: {err}");
        });
        return;
    }

    let expected = std::fs::read_to_string(&generated_file).unwrap_or_else(|err| {
        panic!("reading `{stem}` failed (run `make update-generated`): {err}");
    });
    assert_eq!(
        generated, expected,
        "generated output for `{stem}` drifted from tests/generated/{stem}.rs; \
         run `make update-generated` if this change is intentional",
    );
}

/// Emit one `#[test]` per supported fixture (so each shows in the test output)
/// plus a `GENERATED_TEST_STEMS` catalogue used to guard against drift. The stem
/// list must match the supported fixtures in [`TEST_TABLE`] — enforced by
/// [`generated_tests_cover_supported_fixtures`].
macro_rules! generated_tests {
    ($($stem:ident),+ $(,)?) => {
        $(
            #[test]
            fn $stem() {
                assert_generated_matches(stringify!($stem));
            }
        )+

        const GENERATED_TEST_STEMS: &[&str] = &[$(stringify!($stem)),+];
    };
}

generated_tests!(
    allof_merge,
    anyof_untagged,
    array_types,
    ext_x_rust_type,
    freeform_any,
    integer_formats,
    map_alias,
    metadata_docs,
    nullable,
    number_formats,
    object_additional_properties,
    object_nested_inline,
    object_optional_required,
    oneof_discriminator,
    oneof_untagged,
    primitive_scalars,
    ref_local,
    string_enum,
    string_formats,
);

/// The generated `#[test]`s must cover exactly the supported fixtures, so a new
/// supported fixture cannot be added without its own `#[test]`.
#[test]
fn generated_tests_cover_supported_fixtures() {
    let covered: BTreeSet<&str> = GENERATED_TEST_STEMS.iter().copied().collect();
    assert_eq!(
        covered,
        fixtures_with(Status::Supported),
        "the `generated_tests!` list is out of sync with the supported fixtures in TEST_TABLE",
    );
}

/// Configuration that enables the axum server generator.
fn server_config() -> oapi_codegen::Config {
    let mut import_mapping = std::collections::BTreeMap::new();
    import_mapping.insert("schemas/widgets.yaml".to_owned(), "crate::apimodel".to_owned());
    import_mapping.insert("schemas/shared.yaml".to_owned(), "crate::apimodel".to_owned());
    return oapi_codegen::Config {
        generate: oapi_codegen::config::Generate {
            std_http_server: true,
            ..Default::default()
        },
        import_mapping,
        ..Default::default()
    };
}

/// Regenerate `stem`'s server output and assert it matches the committed file.
///
/// Refresh the committed files after an intentional change with
/// `make update-generated`
/// (`UPDATE_GENERATED=1 cargo test -p oapi-codegen --test coverage`).
fn assert_server_generated_matches(stem: &str) {
    let dir = tests_dir();
    let fixture = dir.join("fixtures").join(format!("{stem}.yaml"));
    let generated_file = dir.join("generated").join(format!("{stem}.rs"));

    let generated = oapi_codegen::generate(&fixture, &server_config()).unwrap_or_else(|err| {
        panic!("generating server `{stem}` failed: {err}");
    });

    if std::env::var_os("UPDATE_GENERATED").is_some() {
        std::fs::write(&generated_file, &generated).unwrap_or_else(|err| {
            panic!("writing `{stem}` failed: {err}");
        });
        return;
    }

    let expected = std::fs::read_to_string(&generated_file).unwrap_or_else(|err| {
        panic!("reading `{stem}` failed (run `make update-generated`): {err}");
    });
    assert_eq!(
        generated, expected,
        "generated server output for `{stem}` drifted from tests/generated/{stem}.rs; \
         run `make update-generated` if this change is intentional",
    );
}

/// Emit one `#[test]` per server fixture (so each shows in the test output) plus
/// a `SERVER_GENERATED_TEST_STEMS` catalogue used to guard against drift. The
/// stem list must match [`SERVER_FIXTURES`] — enforced by
/// [`server_generated_tests_cover_server_fixtures`].
macro_rules! server_generated_tests {
    ($($stem:ident),+ $(,)?) => {
        $(
            #[test]
            fn $stem() {
                assert_server_generated_matches(stringify!($stem));
            }
        )+

        const SERVER_GENERATED_TEST_STEMS: &[&str] = &[$(stringify!($stem)),+];
    };
}

server_generated_tests!(
    server_petstore,
    server_refs,
    server_query_params,
    server_header_params,
    server_default_range_responses,
    server_cookie_params,
    server_component_param_ref,
    server_component_body_ref,
    server_component_param_ref_pet,
    server_xfile_refs,
    server_response_headers,
);

/// The server `#[test]`s must cover exactly the supported server fixtures.
#[test]
fn server_generated_tests_cover_server_fixtures() {
    let covered: BTreeSet<&str> = SERVER_GENERATED_TEST_STEMS.iter().copied().collect();
    let expected: BTreeSet<&str> = SERVER_FIXTURES.iter().copied().collect();
    assert_eq!(
        covered, expected,
        "the `server_generated_tests!` list is out of sync with SERVER_FIXTURES",
    );
}

/// Unsupported server fixtures must be rejected with an error, never silently
/// mishandled — covering the slice's documented limitations.
#[test]
fn server_unsupported_features_are_rejected() {
    let dir = tests_dir();
    for stem in SERVER_UNSUPPORTED_FIXTURES {
        let fixture = dir.join("fixtures").join(format!("{stem}.yaml"));
        let result = oapi_codegen::generate(&fixture, &server_config());
        assert!(
            result.is_err(),
            "`{stem}` is catalogued as unsupported but server generation succeeded",
        );
    }
}

/// Every supported generated file must be `include!`d by
/// `tests/generated_compiles.rs` so its emitted code is type-checked against
/// real serde/chrono/uuid.
#[test]
fn generated_outputs_are_compile_checked() {
    let source = include_str!("generated_compiles.rs");
    let stems = fixtures_with(Status::Supported)
        .into_iter()
        .chain(SERVER_FIXTURES.iter().copied());
    for stem in stems {
        let needle = format!("include!(\"generated/{stem}.rs\")");
        assert!(
            source.contains(&needle),
            "tests/generated_compiles.rs does not compile-check `{stem}`; \
             add a `pub mod {stem} {{ {needle}; }}`",
        );
    }
}

/// Unsupported fixtures must be rejected with an error, never silently mishandled.
#[test]
fn unsupported_features_are_rejected() {
    let dir = tests_dir();
    for stem in fixtures_with(Status::Unsupported) {
        let fixture = dir.join("fixtures").join(format!("{stem}.yaml"));
        let result = oapi_codegen::generate_models_string(&fixture);
        assert!(
            result.is_err(),
            "`{stem}` is catalogued as unsupported but generation succeeded",
        );
    }
}

/// Every fixture on disk must be catalogued, and every catalogued fixture must
/// exist — so nothing is tested without a recorded status and nothing is
/// recorded without being tested.
#[test]
fn fixtures_and_test_table_agree() {
    let dir = tests_dir();
    let fixtures = dir.join("fixtures");

    let referenced: BTreeSet<&str> = TEST_TABLE
        .iter()
        .filter_map(|feature| {
            return feature.fixture;
        })
        .chain(SERVER_FIXTURES.iter().copied())
        .chain(SERVER_UNSUPPORTED_FIXTURES.iter().copied())
        .collect();

    for stem in &referenced {
        let path = fixtures.join(format!("{stem}.yaml"));
        assert!(path.exists(), "TEST_TABLE references missing fixture `{stem}.yaml`");
    }

    let entries = std::fs::read_dir(&fixtures).expect("read fixtures dir");
    for entry in entries {
        let path = entry.expect("dir entry").path();
        let is_yaml = path.extension().is_some_and(|ext| {
            return ext == "yaml";
        });
        if !is_yaml {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| {
                return s.to_str();
            })
            .expect("fixture stem");
        assert!(
            referenced.contains(stem),
            "fixture `{stem}.yaml` is not catalogued in TEST_TABLE",
        );
    }
}

/// The test table itself must be well-formed: unique elements, and every
/// supported/unsupported row backed by a fixture.
#[test]
fn test_table_is_well_formed() {
    let mut seen = BTreeSet::new();
    for feature in TEST_TABLE {
        assert!(
            seen.insert(feature.element),
            "duplicate TEST_TABLE element `{}`",
            feature.element,
        );
        match feature.status {
            Status::Supported | Status::Unsupported => {
                assert!(
                    feature.fixture.is_some(),
                    "element `{}` is {:?} but has no fixture",
                    feature.element,
                    feature.status,
                );
            }
            Status::Ignored | Status::Planned => {}
        }
    }
}

/// Asserts that every `openapiv3` schema enum variant is mapped to a catalogued
/// test-table element. Combined with the wildcard-free matches in [`anchor`], this
/// makes it impossible for a new upstream variant to slip through uncatalogued:
/// the match stops compiling, and this test fails if its element is missing.
#[test]
fn every_schema_variant_is_catalogued() {
    let elements: BTreeSet<&str> = TEST_TABLE
        .iter()
        .map(|feature| {
            return feature.element;
        })
        .collect();

    for element in anchor::all_elements() {
        assert!(
            elements.contains(element),
            "openapiv3 variant maps to `{element}`, which is missing from TEST_TABLE",
        );
    }
}

/// Wildcard-free matches over every `openapiv3` schema enum.
///
/// These exist purely to pin the set of OpenAPI constructs we know about: if a
/// future `openapiv3` release adds an enum variant, the corresponding match
/// becomes non-exhaustive and the crate fails to compile until the variant is
/// handled and added to [`TEST_TABLE`].
mod anchor {
    use openapiv3::AdditionalProperties;
    use openapiv3::AnySchema;
    use openapiv3::ArrayType;
    use openapiv3::BooleanType;
    use openapiv3::IntegerFormat;
    use openapiv3::IntegerType;
    use openapiv3::NumberFormat;
    use openapiv3::NumberType;
    use openapiv3::ObjectType;
    use openapiv3::ReferenceOr;
    use openapiv3::SchemaKind;
    use openapiv3::StringFormat;
    use openapiv3::StringType;
    use openapiv3::Type;

    /// Test-table element for a [`SchemaKind`] variant.
    fn schema_kind_element(kind: &SchemaKind) -> &'static str {
        match kind {
            SchemaKind::Type(ty) => {
                return type_element(ty);
            }
            SchemaKind::OneOf { .. } => {
                return "schema.oneOf";
            }
            SchemaKind::AllOf { .. } => {
                return "schema.allOf";
            }
            SchemaKind::AnyOf { .. } => {
                return "schema.anyOf";
            }
            SchemaKind::Not { .. } => {
                return "schema.not";
            }
            SchemaKind::Any(_) => {
                return "schema.any";
            }
        }
    }

    /// Test-table element for a [`Type`] variant.
    fn type_element(ty: &Type) -> &'static str {
        match ty {
            Type::String(_) => {
                return "schema.type.string";
            }
            Type::Number(_) => {
                return "schema.type.number";
            }
            Type::Integer(_) => {
                return "schema.type.integer";
            }
            Type::Object(_) => {
                return "schema.type.object";
            }
            Type::Array(_) => {
                return "schema.type.array";
            }
            Type::Boolean(_) => {
                return "schema.type.boolean";
            }
        }
    }

    /// Test-table element for a [`StringFormat`] variant.
    fn string_format_element(format: &StringFormat) -> &'static str {
        match format {
            StringFormat::Date => {
                return "format.string.date";
            }
            StringFormat::DateTime => {
                return "format.string.date-time";
            }
            StringFormat::Password => {
                return "format.string.password";
            }
            StringFormat::Byte => {
                return "format.string.byte";
            }
            StringFormat::Binary => {
                return "format.string.binary";
            }
        }
    }

    /// Test-table element for an [`IntegerFormat`] variant.
    fn integer_format_element(format: &IntegerFormat) -> &'static str {
        match format {
            IntegerFormat::Int32 => {
                return "format.integer.int32";
            }
            IntegerFormat::Int64 => {
                return "format.integer.int64";
            }
        }
    }

    /// Test-table element for a [`NumberFormat`] variant.
    fn number_format_element(format: &NumberFormat) -> &'static str {
        match format {
            NumberFormat::Float => {
                return "format.number.float";
            }
            NumberFormat::Double => {
                return "format.number.double";
            }
        }
    }

    /// Test-table element for an [`AdditionalProperties`] variant.
    fn additional_properties_element(value: &AdditionalProperties) -> &'static str {
        match value {
            AdditionalProperties::Any(true) => {
                return "object.additionalProperties.true";
            }
            AdditionalProperties::Any(false) => {
                return "object.additionalProperties.false";
            }
            AdditionalProperties::Schema(_) => {
                return "object.additionalProperties.schema";
            }
        }
    }

    /// Collect the test-table element for every variant of every catalogued enum, by
    /// constructing one value per variant. This both exercises the wildcard-free
    /// matches and feeds the catalogue cross-check, so a new upstream enum
    /// variant cannot pass without a corresponding [`super::TEST_TABLE`] row.
    pub fn all_elements() -> Vec<&'static str> {
        let dummy_ref = || {
            return ReferenceOr::Reference {
                reference: String::new(),
            };
        };
        let kinds = [
            SchemaKind::Type(Type::String(StringType::default())),
            SchemaKind::Type(Type::Number(NumberType::default())),
            SchemaKind::Type(Type::Integer(IntegerType::default())),
            SchemaKind::Type(Type::Object(ObjectType::default())),
            SchemaKind::Type(Type::Array(ArrayType {
                items: None,
                min_items: None,
                max_items: None,
                unique_items: false,
            })),
            SchemaKind::Type(Type::Boolean(BooleanType::default())),
            SchemaKind::OneOf { one_of: Vec::new() },
            SchemaKind::AllOf { all_of: Vec::new() },
            SchemaKind::AnyOf { any_of: Vec::new() },
            SchemaKind::Not {
                not: Box::new(dummy_ref()),
            },
            SchemaKind::Any(AnySchema::default()),
        ];
        let string_formats = [
            StringFormat::Date,
            StringFormat::DateTime,
            StringFormat::Password,
            StringFormat::Byte,
            StringFormat::Binary,
        ];
        let integer_formats = [IntegerFormat::Int32, IntegerFormat::Int64];
        let number_formats = [NumberFormat::Float, NumberFormat::Double];
        let additional = [
            AdditionalProperties::Any(true),
            AdditionalProperties::Any(false),
            AdditionalProperties::Schema(Box::new(dummy_ref())),
        ];

        let mut elements = Vec::new();
        for kind in &kinds {
            elements.push(schema_kind_element(kind));
        }
        for format in &string_formats {
            elements.push(string_format_element(format));
        }
        for format in &integer_formats {
            elements.push(integer_format_element(format));
        }
        for format in &number_formats {
            elements.push(number_format_element(format));
        }
        for value in &additional {
            elements.push(additional_properties_element(value));
        }
        return elements;
    }
}
