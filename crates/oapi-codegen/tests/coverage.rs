//! Single source of truth for OpenAPI 3.0 schema-feature coverage.
//!
//! For every OpenAPI 3.0 construct the models generator must do exactly one of:
//! generate Rust ([`Status::Supported`]), reject it with a documented error
//! ([`Status::Unsupported`]), recognise but intentionally drop it
//! ([`Status::Ignored`]), or defer it to server/client generation
//! ([`Status::Planned`]). Nothing can be left in the unknown.
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
        element: "ext.x-rust-name",
        status: Status::Supported,
        fixture: Some("ext_vendor_extensions"),
    },
    Feature {
        element: "ext.x-rust-serde-skip",
        status: Status::Supported,
        fixture: Some("ext_vendor_extensions"),
    },
    Feature {
        element: "ext.x-order",
        status: Status::Supported,
        fixture: Some("ext_vendor_extensions"),
    },
    Feature {
        element: "ext.x-enum-varnames",
        status: Status::Supported,
        fixture: Some("ext_vendor_extensions"),
    },
    Feature {
        element: "ext.x-deprecated-reason",
        status: Status::Supported,
        fixture: Some("ext_vendor_extensions"),
    },
    Feature {
        element: "ext.x-omitempty",
        status: Status::Supported,
        fixture: Some("ext_vendor_extensions"),
    },
    Feature {
        element: "ext.x-go-*",
        status: Status::Ignored,
        fixture: None,
    },
    // Naming
    Feature {
        element: "naming.type-collisions",
        status: Status::Supported,
        fixture: Some("type_name_collisions"),
    },
    // Document-level (server/client generation). The axum server generator now
    // covers a slice of paths/parameters/requestBody/responses; the blocking
    // reqwest client generator additionally covers securitySchemes. Those slices
    // are validated separately by `SERVER_FIXTURES` and `CLIENT_FIXTURES`. The
    // rest remains deferred.
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
    "server_text_body",
    "server_form_body",
    "server_multipart_body",
    "server_json_charset",
    "server_multi_content_request",
    "server_multi_content_response",
    "server_prune",
    "server_filtering",
    "server_urls",
];

/// Server fixtures whose generation must fail with a documented error, covering
/// the slice's deliberate limitations (for example an unrecognized HTTP status code).
const SERVER_UNSUPPORTED_FIXTURES: &[&str] = &[
    "server_unsupported_unknown_status",
    "server_unsupported_xfile_response_ref",
    "server_unsupported_object_path_param",
    "server_unsupported_path_param_not_in_template",
    "server_unsupported_undeclared_path_param",
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
    "server_unsupported_colliding_response_headers",
    "server_unsupported_ref_response_header",
    "server_unsupported_reserved_response_header",
    "server_unsupported_only_binary_body",
    "server_unsupported_text_non_string_body",
    "server_unsupported_form_scalar_body",
    "server_unsupported_form_ref_scalar_body",
    "server_unsupported_multipart_nonobject_body",
    "server_unsupported_multipart_object_field",
    "server_unsupported_multipart_xfile_field",
    "server_unsupported_multipart_xfile_body",
    "server_unsupported_multipart_combined",
];

/// Client fixtures whose generated blocking `reqwest` client is compile-checked
/// against a committed file. These exercise the client generator: path/query/
/// header/cookie inputs, single-content request bodies, typed responses, and
/// security schemes (bearer, basic, and API-key credentials).
const CLIENT_FIXTURES: &[&str] = &[
    "client_widgets",
    "client_auth",
    "client_multipart_request",
    "client_negotiated_request",
    "client_negotiated_response",
    "client_form_response",
];

/// Client fixtures whose generation must fail with a documented error, covering
/// the request/response shapes the client generator does not support yet.
const CLIENT_UNSUPPORTED_FIXTURES: &[&str] = &[
    "client_unsupported_oauth2",
    "client_unsupported_ref_scheme",
    "client_unsupported_undeclared_scheme",
];

/// Fixtures generated with both the server and client enabled, exercising the
/// flat crate-root layout in which the server and client share one file and the
/// same per-operation types alongside the component models.
const COMBINED_FIXTURES: &[&str] = &["combined_server_client", "combined_response_name_collision"];

/// Combined fixtures whose generation must fail because a component schema is
/// named like a crate-root interface type the flat layout emits (`Api`,
/// `Client`, `ClientError`). Each is rejected with a `TypeNameCollision`.
const COMBINED_UNSUPPORTED_FIXTURES: &[&str] = &[
    "combined_reserved_name_api",
    "combined_reserved_name_client",
    "combined_reserved_name_client_error",
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
    ext_vendor_extensions,
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
    type_name_collisions,
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
            server_urls: true,
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
    server_text_body,
    server_form_body,
    server_multipart_body,
    server_json_charset,
    server_multi_content_request,
    server_multi_content_response,
    server_prune,
    server_filtering,
    server_urls,
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

/// Unused component schemas are pruned by default, but retained when
/// `output-options.skip-prune` is set — mirroring `oapi-codegen`'s pruning.
#[test]
fn skip_prune_retains_unused_schemas() {
    let fixture = tests_dir().join("fixtures").join("server_prune.yaml");

    let pruned = oapi_codegen::generate(&fixture, &server_config()).expect("generating pruned server output failed");
    assert!(
        pruned.contains("struct Thing") && pruned.contains("struct Child"),
        "schemas reachable from an operation must survive pruning",
    );
    assert!(
        !pruned.contains("Unreferenced") && !pruned.contains("OnlyViaUnreferenced"),
        "schemas no operation references must be pruned by default",
    );

    let mut config = server_config();
    config.output_options.skip_prune = true;
    let unpruned = oapi_codegen::generate(&fixture, &config).expect("generating unpruned server output failed");
    assert!(
        unpruned.contains("struct Unreferenced") && unpruned.contains("struct OnlyViaUnreferenced"),
        "skip-prune must retain schemas no operation references",
    );
}

/// Path to the shared filtering fixture (three tagged/untagged operations).
fn filtering_fixture() -> PathBuf {
    return tests_dir().join("fixtures").join("server_filtering.yaml");
}

/// `exclude-tags` drops operations carrying an excluded tag, and pruning then
/// removes any component schema the dropped operation uniquely referenced.
#[test]
fn filter_exclude_tags_drops_tagged_operations() {
    let mut config = server_config();
    config.output_options.exclude_tags = vec!["admin".to_owned()];

    let generated =
        oapi_codegen::generate(&filtering_fixture(), &config).expect("generating filtered server output failed");
    assert!(
        generated.contains("fn list_pets") && generated.contains("fn health_check"),
        "operations without the excluded tag must be kept",
    );
    assert!(
        !generated.contains("fn get_stats"),
        "operations carrying an excluded tag must be dropped",
    );
    assert!(
        generated.contains("struct Pet") && !generated.contains("struct Stats"),
        "a schema referenced only by a dropped operation must be pruned",
    );
}

/// `include-tags` keeps only operations carrying one of the included tags;
/// untagged operations are dropped.
#[test]
fn filter_include_tags_keeps_only_tagged_operations() {
    let mut config = server_config();
    config.output_options.include_tags = vec!["pets".to_owned()];

    let generated =
        oapi_codegen::generate(&filtering_fixture(), &config).expect("generating filtered server output failed");
    assert!(
        generated.contains("fn list_pets"),
        "operations with an included tag must be kept",
    );
    assert!(
        !generated.contains("fn get_stats") && !generated.contains("fn health_check"),
        "operations without an included tag (including untagged) must be dropped",
    );
}

/// `exclude-operation-ids` drops operations by `operationId`; the rest survive.
#[test]
fn filter_exclude_operation_ids_drops_named_operations() {
    let mut config = server_config();
    config.output_options.exclude_operation_ids = vec!["healthCheck".to_owned()];

    let generated =
        oapi_codegen::generate(&filtering_fixture(), &config).expect("generating filtered server output failed");
    assert!(
        generated.contains("fn list_pets") && generated.contains("fn get_stats"),
        "operations not named in exclude-operation-ids must be kept",
    );
    assert!(
        !generated.contains("fn health_check"),
        "operations named in exclude-operation-ids must be dropped",
    );
}

/// `include-operation-ids` keeps only operations whose `operationId` is listed.
#[test]
fn filter_include_operation_ids_keeps_only_named_operations() {
    let mut config = server_config();
    config.output_options.include_operation_ids = vec!["listPets".to_owned()];

    let generated =
        oapi_codegen::generate(&filtering_fixture(), &config).expect("generating filtered server output failed");
    assert!(
        generated.contains("fn list_pets"),
        "operations named in include-operation-ids must be kept",
    );
    assert!(
        !generated.contains("fn get_stats") && !generated.contains("fn health_check"),
        "operations not named in include-operation-ids must be dropped",
    );
}

/// `exclude-schemas` removes named component schemas from models generation,
/// leaving the rest intact. Models-only generation never prunes, so a negative
/// control (generation without the option) proves the removal is attributable to
/// `exclude-schemas` and not to pruning.
#[test]
fn filter_exclude_schemas_removes_named_models() {
    let models_only = oapi_codegen::config::Generate {
        models: true,
        ..Default::default()
    };

    let baseline = oapi_codegen::Config {
        generate: models_only.clone(),
        ..Default::default()
    };
    let unfiltered =
        oapi_codegen::generate(&filtering_fixture(), &baseline).expect("generating baseline models failed");
    assert!(
        unfiltered.contains("struct Standalone"),
        "without exclude-schemas the schema must be generated (models-only never prunes)",
    );

    let config = oapi_codegen::Config {
        generate: models_only,
        output_options: oapi_codegen::config::OutputOptions {
            exclude_schemas: vec!["Standalone".to_owned()],
            ..Default::default()
        },
        ..Default::default()
    };
    let generated = oapi_codegen::generate(&filtering_fixture(), &config).expect("generating filtered models failed");
    assert!(
        generated.contains("struct Pet") && generated.contains("struct Stats"),
        "schemas not named in exclude-schemas must be generated",
    );
    assert!(
        !generated.contains("struct Standalone"),
        "schemas named in exclude-schemas must not be generated",
    );
}

/// Path to the shared server-URLs fixture (three servers: a const, a builder
/// with an enum variable, and an `x-rust-name` server).
fn server_urls_fixture() -> PathBuf {
    return tests_dir().join("fixtures").join("server_urls.yaml");
}

/// `server-urls` is opt-in: without the flag no server constants or builders are
/// emitted, even when the spec declares `servers:`.
#[test]
fn server_urls_are_not_emitted_without_the_flag() {
    let config = oapi_codegen::Config {
        generate: oapi_codegen::config::Generate {
            std_http_server: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let generated = oapi_codegen::generate(&server_urls_fixture(), &config).expect("generating server output failed");
    assert!(
        !generated.contains("SERVER_URL_PRODUCTION") && !generated.contains("fn server_url_regional"),
        "server URLs must not be emitted unless `generate.server-urls` is set",
    );
}

/// `server-urls` is independent of the models/server/client artifacts: it is
/// emitted for a models-only configuration too.
#[test]
fn server_urls_emit_in_models_only_mode() {
    let config = oapi_codegen::Config {
        generate: oapi_codegen::config::Generate {
            models: true,
            server_urls: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let generated = oapi_codegen::generate(&server_urls_fixture(), &config).expect("generating models output failed");
    assert!(
        generated.contains("pub const SERVER_URL_PRODUCTION: &str = \"https://api.example.com/v1\";"),
        "the variable-free server must emit a string constant",
    );
    assert!(
        generated.contains("pub fn server_url_regional(") && generated.contains("port: ServerUrlRegionalPort"),
        "a server with an enum variable must emit a builder taking the enum type",
    );
    assert!(
        generated.contains("pub fn sandbox("),
        "`x-rust-name` overrides the derived server identifier",
    );
}

/// `embedded-spec` is not implemented; the library `generate()` must reject it
/// rather than silently ignore the flag (as the CLI already does).
#[test]
fn embedded_spec_is_rejected() {
    let config = oapi_codegen::Config {
        generate: oapi_codegen::config::Generate {
            models: true,
            embedded_spec: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let result = oapi_codegen::generate(&server_urls_fixture(), &config);
    assert!(
        matches!(result, Err(oapi_codegen::Error::Unimplemented(mode)) if mode == "embedded-spec"),
        "generate() must reject the unimplemented `embedded-spec` flag",
    );
}

/// Configuration that enables the blocking `reqwest` client generator.
fn client_config() -> oapi_codegen::Config {
    return oapi_codegen::Config {
        generate: oapi_codegen::config::Generate {
            client: true,
            ..Default::default()
        },
        ..Default::default()
    };
}

/// Regenerate `stem`'s client output and assert it matches the committed file.
///
/// Refresh the committed files after an intentional change with
/// `make update-generated`
/// (`UPDATE_GENERATED=1 cargo test -p oapi-codegen --test coverage`).
fn assert_client_generated_matches(stem: &str) {
    let dir = tests_dir();
    let fixture = dir.join("fixtures").join(format!("{stem}.yaml"));
    let generated_file = dir.join("generated").join(format!("{stem}.rs"));

    let generated = oapi_codegen::generate(&fixture, &client_config()).unwrap_or_else(|err| {
        panic!("generating client `{stem}` failed: {err}");
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
        "generated client output for `{stem}` drifted from tests/generated/{stem}.rs; \
         run `make update-generated` if this change is intentional",
    );
}

/// Emit one `#[test]` per client fixture (so each shows in the test output) plus
/// a `CLIENT_GENERATED_TEST_STEMS` catalogue used to guard against drift. The
/// stem list must match [`CLIENT_FIXTURES`] — enforced by
/// [`client_generated_tests_cover_client_fixtures`].
macro_rules! client_generated_tests {
    ($($stem:ident),+ $(,)?) => {
        $(
            #[test]
            fn $stem() {
                assert_client_generated_matches(stringify!($stem));
            }
        )+

        const CLIENT_GENERATED_TEST_STEMS: &[&str] = &[$(stringify!($stem)),+];
    };
}

client_generated_tests!(
    client_widgets,
    client_auth,
    client_multipart_request,
    client_negotiated_request,
    client_negotiated_response,
    client_form_response,
);

/// The client `#[test]`s must cover exactly the supported client fixtures.
#[test]
fn client_generated_tests_cover_client_fixtures() {
    let covered: BTreeSet<&str> = CLIENT_GENERATED_TEST_STEMS.iter().copied().collect();
    let expected: BTreeSet<&str> = CLIENT_FIXTURES.iter().copied().collect();
    assert_eq!(
        covered, expected,
        "the `client_generated_tests!` list is out of sync with CLIENT_FIXTURES",
    );
}

/// Unsupported client fixtures must be rejected with an error, never silently
/// mishandled — covering the request/response shapes the client cannot emit yet.
#[test]
fn client_unsupported_features_are_rejected() {
    let dir = tests_dir();
    for stem in CLIENT_UNSUPPORTED_FIXTURES {
        let fixture = dir.join("fixtures").join(format!("{stem}.yaml"));
        let result = oapi_codegen::generate(&fixture, &client_config());
        assert!(
            result.is_err(),
            "`{stem}` is catalogued as unsupported but client generation succeeded",
        );
    }
}

/// Configuration that enables both the axum server and the `reqwest` client, so
/// they are emitted together flat at the crate root.
fn combined_config() -> oapi_codegen::Config {
    return oapi_codegen::Config {
        generate: oapi_codegen::config::Generate {
            std_http_server: true,
            client: true,
            ..Default::default()
        },
        ..Default::default()
    };
}

/// The combined configuration for `stem`. `combined_response_name_collision`
/// carries a schema named `GetWidgetResponse` that clashes with the generated
/// response enum, so it sets `response-type-suffix` to `Resp` to move the enum
/// aside (`GetWidgetResp`); every other fixture uses the default suffix.
fn combined_config_for(stem: &str) -> oapi_codegen::Config {
    let mut config = combined_config();
    if stem == "combined_response_name_collision" {
        config.output_options.response_type_suffix = Some("Resp".to_owned());
    }
    return config;
}

/// Regenerate `stem`'s combined server+client output and assert it matches the
/// committed file.
///
/// Refresh the committed files after an intentional change with
/// `make update-generated`
/// (`UPDATE_GENERATED=1 cargo test -p oapi-codegen --test coverage`).
fn assert_combined_generated_matches(stem: &str) {
    let dir = tests_dir();
    let fixture = dir.join("fixtures").join(format!("{stem}.yaml"));
    let generated_file = dir.join("generated").join(format!("{stem}.rs"));

    let generated = oapi_codegen::generate(&fixture, &combined_config_for(stem)).unwrap_or_else(|err| {
        panic!("generating combined `{stem}` failed: {err}");
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
        "generated combined output for `{stem}` drifted from tests/generated/{stem}.rs; \
         run `make update-generated` if this change is intentional",
    );
}

/// Emit one `#[test]` per combined fixture (so each shows in the test output)
/// plus a `COMBINED_GENERATED_TEST_STEMS` catalogue used to guard against drift.
/// The stem list must match [`COMBINED_FIXTURES`] — enforced by
/// [`combined_generated_tests_cover_combined_fixtures`].
macro_rules! combined_generated_tests {
    ($($stem:ident),+ $(,)?) => {
        $(
            #[test]
            fn $stem() {
                assert_combined_generated_matches(stringify!($stem));
            }
        )+

        const COMBINED_GENERATED_TEST_STEMS: &[&str] = &[$(stringify!($stem)),+];
    };
}

combined_generated_tests!(combined_server_client, combined_response_name_collision);

/// Without `response-type-suffix`, a schema named like an operation's response
/// enum must fail generation rather than silently rename either item. The hint
/// leads with the surgical `x-rust-name` remedy before the broad suffix option.
#[test]
fn response_name_collision_without_suffix_fails() {
    let dir = tests_dir();
    let fixture = dir.join("fixtures").join("combined_response_name_collision.yaml");
    let err = oapi_codegen::generate(&fixture, &combined_config())
        .expect_err("expected a type-name collision without response-type-suffix");
    let oapi_codegen::Error::TypeNameCollision { hint, .. } = &err else {
        panic!("expected TypeNameCollision, got: {err:?}");
    };
    let x_rust_name = hint.find("x-rust-name").expect("hint should mention x-rust-name");
    let suffix = hint
        .find("response-type-suffix")
        .expect("hint should mention response-type-suffix");
    assert!(
        x_rust_name < suffix,
        "hint should lead with the surgical `x-rust-name` remedy before `response-type-suffix`, got: {hint}",
    );
}

/// A parameter declared `in: path` with no matching `{placeholder}` in the path
/// template must fail generation with a guided error rather than silently drop
/// the parameter from the generated signature.
#[test]
fn path_param_not_in_template_is_rejected() {
    let dir = tests_dir();
    let fixture = dir
        .join("fixtures")
        .join("server_unsupported_path_param_not_in_template.yaml");
    let err = oapi_codegen::generate(&fixture, &server_config())
        .expect_err("a path parameter missing from the template must be rejected");
    assert!(
        matches!(&err, oapi_codegen::Error::InvalidPathParameter { name, .. } if name == "tz"),
        "expected InvalidPathParameter for `tz`, got: {err:?}",
    );
    let message = err.to_string();
    assert!(
        message.contains("`tz`") && message.contains("{tz}"),
        "error should name the parameter and the missing placeholder, got: {message}",
    );
}

/// A `{placeholder}` in the path template with no declared `in: path` parameter
/// must fail generation with a guided error rather than silently assume the
/// parameter is a `String`, matching Go `oapi-codegen`'s bidirectional strictness.
#[test]
fn undeclared_path_placeholder_is_rejected() {
    let dir = tests_dir();
    let fixture = dir
        .join("fixtures")
        .join("server_unsupported_undeclared_path_param.yaml");
    let err = oapi_codegen::generate(&fixture, &server_config())
        .expect_err("a path placeholder with no declared parameter must be rejected");
    assert!(
        matches!(&err, oapi_codegen::Error::UndeclaredPathParameter { name, .. } if name == "id"),
        "expected UndeclaredPathParameter for `id`, got: {err:?}",
    );
    let message = err.to_string();
    assert!(
        message.contains("{id}") && message.contains("in: path"),
        "error should name the placeholder and the missing declaration, got: {message}",
    );
}

/// An empty `response-type-suffix` is treated as unset: it must fall back to
/// the default suffix rather than emit suffix-less response enums, so the
/// collision fixture fails exactly as it does without any suffix.
#[test]
fn empty_response_suffix_falls_back_to_default() {
    let dir = tests_dir();
    let fixture = dir.join("fixtures").join("combined_response_name_collision.yaml");
    let mut config = combined_config();
    config.output_options.response_type_suffix = Some(String::new());
    let err = oapi_codegen::generate(&fixture, &config)
        .expect_err("an empty response-type-suffix must fall back to the default and still collide");
    assert!(
        matches!(err, oapi_codegen::Error::TypeNameCollision { .. }),
        "expected TypeNameCollision, got: {err:?}",
    );
}

/// The dependency report must reflect real generated output: a combined
/// server+client references at least serde/http/axum/reqwest, and every crate it
/// names is one the generator can actually emit. This ties `required_dependencies`
/// to emitted code (not just synthetic strings), so a new crate the emitters
/// start referencing — which will also force a new dev-dependency to compile the
/// goldens — is a prompt to extend the report.
#[test]
fn dependency_report_reflects_generated_output() {
    let fixture = tests_dir().join("fixtures").join("combined_server_client.yaml");
    let code = oapi_codegen::generate(&fixture, &combined_config()).expect("combined generation failed");
    let names: Vec<&str> = oapi_codegen::deps::required_dependencies(&code)
        .iter()
        .map(|dep| return dep.name)
        .collect();

    for expected in ["serde", "http", "axum", "reqwest"] {
        assert!(
            names.contains(&expected),
            "report is missing `{expected}`; got {names:?}"
        );
    }

    const KNOWN: &[&str] = &[
        "serde",
        "serde_json",
        "chrono",
        "uuid",
        "http",
        "axum",
        "axum-extra",
        "reqwest",
        "percent-encoding",
        "serde_urlencoded",
    ];
    for name in &names {
        assert!(KNOWN.contains(name), "report named an unexpected crate `{name}`");
    }
}

/// with the same name at the crate root.
#[test]
fn reserved_interface_name_collision_fails() {
    let dir = tests_dir();
    let cases = [
        ("combined_reserved_name_api", "Api", "server interface trait"),
        ("combined_reserved_name_client", "Client", "client struct"),
        (
            "combined_reserved_name_client_error",
            "ClientError",
            "client error enum",
        ),
    ];
    for (stem, schema_name, artifact) in cases {
        let fixture = dir.join("fixtures").join(format!("{stem}.yaml"));
        let err = oapi_codegen::generate(&fixture, &combined_config())
            .expect_err(&format!("expected a collision for schema named `{schema_name}`"));
        match err {
            oapi_codegen::Error::TypeNameCollision {
                name, artifact: got, ..
            } => {
                assert_eq!(name, schema_name);
                assert_eq!(got, artifact);
            }
            other => panic!("expected TypeNameCollision for `{schema_name}`, got: {other:?}"),
        }
    }
}

/// A reserved name is only reserved when its target is requested: the
/// server-only `Api` trait must not block a schema named `Api` in a
/// client-only generation.
#[test]
fn reserved_interface_name_is_target_scoped() {
    let fixture = tests_dir().join("fixtures").join("combined_reserved_name_api.yaml");
    oapi_codegen::generate(&fixture, &client_config())
        .expect("a schema named `Api` must not collide when only the client is generated");
}

/// Whether `generated` declares `name` as a `trait`, `struct`, or `enum` item.
/// The match requires an item keyword before the name and a non-identifier
/// character after it, so a longer identifier that merely shares the prefix
/// (e.g. `ClientError` when looking for `Client`) does not count.
fn declares_type(generated: &str, name: &str) -> bool {
    return ["trait", "struct", "enum"].iter().any(|keyword| {
        let needle = format!("{keyword} {name}");
        return generated.match_indices(&needle).any(|(index, matched)| {
            let after = generated[index + matched.len()..].chars().next();
            return after.is_none_or(|next| return !next.is_alphanumeric() && next != '_');
        });
    });
}

/// Every reserved interface name must actually be declared in the combined
/// output, so renaming an emitted interface without updating its reserved-name
/// constant (which will let a real collision slip through) breaks this test.
#[test]
fn reserved_names_are_declared_in_combined_output() {
    let dir = tests_dir();
    let fixture = dir.join("fixtures").join("combined_server_client.yaml");
    let generated =
        oapi_codegen::generate(&fixture, &combined_config()).expect("generating combined server+client output failed");
    let targets = oapi_codegen::emit::Targets {
        server: true,
        client: true,
    };
    for reserved in oapi_codegen::emit::reserved_type_names(targets) {
        assert!(
            declares_type(&generated, reserved.name),
            "reserved name `{}` ({}) is not declared in the combined output",
            reserved.name,
            reserved.description,
        );
    }
}

/// The combined `#[test]`s must cover exactly the combined fixtures.
#[test]
fn combined_generated_tests_cover_combined_fixtures() {
    let covered: BTreeSet<&str> = COMBINED_GENERATED_TEST_STEMS.iter().copied().collect();
    let expected: BTreeSet<&str> = COMBINED_FIXTURES.iter().copied().collect();
    assert_eq!(
        covered, expected,
        "the `combined_generated_tests!` list is out of sync with COMBINED_FIXTURES",
    );
}

/// Every supported generated file must be `include!`d by
/// `tests/generated.rs` so its emitted code is type-checked against
/// real serde/chrono/uuid.
#[test]
fn generated_outputs_are_compile_checked() {
    let source = include_str!("generated.rs");
    let stems = fixtures_with(Status::Supported)
        .into_iter()
        .chain(SERVER_FIXTURES.iter().copied())
        .chain(CLIENT_FIXTURES.iter().copied())
        .chain(COMBINED_FIXTURES.iter().copied());
    for stem in stems {
        let needle = format!("include!(\"generated/{stem}.rs\")");
        assert!(
            source.contains(&needle),
            "tests/generated.rs does not compile-check `{stem}`; \
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
        .chain(CLIENT_FIXTURES.iter().copied())
        .chain(CLIENT_UNSUPPORTED_FIXTURES.iter().copied())
        .chain(COMBINED_FIXTURES.iter().copied())
        .chain(COMBINED_UNSUPPORTED_FIXTURES.iter().copied())
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
