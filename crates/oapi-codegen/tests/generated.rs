//! Compile-checks every *supported* generated output and exercises a few
//! representative types at runtime.
//!
//! Each module reads a generated file with `#[path]` so the test crate fails to
//! build if any emitted code stops compiling against its real dependencies
//! (serde, chrono, uuid, serde_json, axum). The module set is kept in lock-step
//! with the coverage matrix by `generated_outputs_are_compile_checked` in
//! `tests/coverage.rs`.
//!
//! `#[path]` and not `include!`, because each generated file opens with an inner
//! attribute that turns off every lint that is about first-party source, and
//! `include!` cannot carry one. So this file needs no lint exceptions of its own.

mod generated {
    #[path = "allof_merge.rs"]
    pub mod allof_merge;
    #[path = "anyof_untagged.rs"]
    pub mod anyof_untagged;
    #[path = "array_types.rs"]
    pub mod array_types;
    #[path = "client_auth.rs"]
    pub mod client_auth;
    #[path = "client_form_response.rs"]
    pub mod client_form_response;
    #[path = "client_multipart_request.rs"]
    pub mod client_multipart_request;
    #[path = "client_negotiated_request.rs"]
    pub mod client_negotiated_request;
    #[path = "client_negotiated_response.rs"]
    pub mod client_negotiated_response;
    #[path = "client_widgets.rs"]
    pub mod client_widgets;
    #[path = "combined_prelude_value_names.rs"]
    pub mod combined_prelude_value_names;
    #[path = "combined_response_name_collision.rs"]
    pub mod combined_response_name_collision;
    #[path = "combined_server_client.rs"]
    pub mod combined_server_client;
    #[path = "combined_x_rust_derive.rs"]
    pub mod combined_x_rust_derive;
    #[path = "ext_vendor_extensions.rs"]
    pub mod ext_vendor_extensions;
    #[path = "ext_x_rust_derive.rs"]
    pub mod ext_x_rust_derive;
    #[path = "ext_x_rust_type.rs"]
    pub mod ext_x_rust_type;
    #[path = "freeform_any.rs"]
    pub mod freeform_any;
    #[path = "integer_formats.rs"]
    pub mod integer_formats;
    #[path = "map_alias.rs"]
    pub mod map_alias;
    #[path = "metadata_docs.rs"]
    pub mod metadata_docs;
    #[path = "nullable.rs"]
    pub mod nullable;
    #[path = "number_formats.rs"]
    pub mod number_formats;
    #[path = "object_additional_properties.rs"]
    pub mod object_additional_properties;
    #[path = "object_deny_unknown_fields.rs"]
    pub mod object_deny_unknown_fields;

    #[path = "compose_shared.rs"]
    pub mod compose_shared;
    #[path = "object_nested_inline.rs"]
    pub mod object_nested_inline;
    #[path = "object_optional_required.rs"]
    pub mod object_optional_required;
    #[path = "oneof_discriminator.rs"]
    pub mod oneof_discriminator;
    #[path = "oneof_untagged.rs"]
    pub mod oneof_untagged;
    #[path = "oneof_variant_naming.rs"]
    pub mod oneof_variant_naming;
    #[path = "prelude_result_name.rs"]
    pub mod prelude_result_name;
    #[path = "prelude_value_names.rs"]
    pub mod prelude_value_names;
    #[path = "primitive_scalars.rs"]
    pub mod primitive_scalars;
    #[path = "recursive_schema.rs"]
    pub mod recursive_schema;
    #[path = "ref_local.rs"]
    pub mod ref_local;
    #[path = "schema_defaults.rs"]
    pub mod schema_defaults;
    #[path = "server_component_body_ref.rs"]
    pub mod server_component_body_ref;
    #[path = "server_component_param_ref.rs"]
    pub mod server_component_param_ref;
    #[path = "server_component_param_ref_pet.rs"]
    pub mod server_component_param_ref_pet;
    #[path = "server_cookie_params.rs"]
    pub mod server_cookie_params;
    #[path = "server_default_range_responses.rs"]
    pub mod server_default_range_responses;
    #[path = "server_filtering.rs"]
    pub mod server_filtering;
    #[path = "server_form_body.rs"]
    pub mod server_form_body;
    #[path = "server_header_params.rs"]
    pub mod server_header_params;
    #[path = "server_json_charset.rs"]
    pub mod server_json_charset;
    #[path = "server_multi_content_request.rs"]
    pub mod server_multi_content_request;
    #[path = "server_multi_content_response.rs"]
    pub mod server_multi_content_response;
    #[path = "server_multipart_body.rs"]
    pub mod server_multipart_body;
    #[path = "server_petstore.rs"]
    pub mod server_petstore;
    #[path = "server_prune.rs"]
    pub mod server_prune;
    #[path = "server_query_params.rs"]
    pub mod server_query_params;
    #[path = "server_refs.rs"]
    pub mod server_refs;
    #[path = "server_response_headers.rs"]
    pub mod server_response_headers;
    #[path = "server_text_body.rs"]
    pub mod server_text_body;
    #[path = "server_urls.rs"]
    pub mod server_urls;

    #[path = "integer_enum.rs"]
    pub mod integer_enum;
    #[path = "multi_spec_compose.rs"]
    pub mod multi_spec_compose;
    #[path = "server_auth.rs"]
    pub mod server_auth;
    #[path = "server_xfile_refs.rs"]
    pub mod server_xfile_refs;
    #[path = "string_enum.rs"]
    pub mod string_enum;
    #[path = "string_formats.rs"]
    pub mod string_formats;
    #[path = "type_name_collisions.rs"]
    pub mod type_name_collisions;
    #[path = "value_constraints.rs"]
    pub mod value_constraints;
}

/// Stand-in for the foreign types the `ext_x_rust_derive` fixture points its
/// `x-rust-type` targets at (`crate::restricted`).
///
/// Each type here really does lack the traits its `x-rust-derive` key leaves out.
/// That makes the fixture a proof and not a snapshot: if the generator emits a
/// derive a target cannot satisfy, this test crate stops building.
#[allow(
    dead_code,
    reason = "test-only stand-in for the x-rust-derive fixture's foreign targets; nothing here is constructed"
)]
mod restricted {
    /// Declares `Debug` alone.
    #[derive(serde::Serialize, serde::Deserialize, Debug)]
    pub struct Opaque(pub String);

    /// Declares `Debug` and `Clone`.
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct Handle(pub String);

    /// Declares nothing. Serde only.
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct Nothing(pub String);
}

/// Stand-in for the models crate the `server_refs` fixture's `import-mapping`
/// points its cross-file `$ref` bodies at (`crate::apimodel`). Real projects
/// generate this module from the referenced schema file; here a minimal struct
/// proves the emitted `crate::apimodel::CreateWidget` path resolves and that the
/// generated handler can decode it as a JSON body.
#[allow(
    dead_code,
    reason = "test-only stand-in for the models crate the server_refs fixture's import-mapping targets; only CreateWidget is constructed here"
)]
mod apimodel {
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
    pub struct CreateWidget {
        pub name: String,
    }

    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
    pub struct NewThing {
        pub name: String,
    }
}

#[test]
fn untagged_enum_round_trips_through_serde() {
    use generated::oneof_discriminator;

    let payment = oneof_discriminator::Payment::Card(oneof_discriminator::Card {
        number: "4111".to_owned(),
    });
    let json = serde_json::to_string(&payment).expect("serialize");
    let back: oneof_discriminator::Payment = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(payment, back);
}

#[test]
fn optional_field_is_skipped_when_none() {
    use generated::object_optional_required;

    let profile = object_optional_required::Profile {
        id: "u1".to_owned(),
        nickname: None,
    };
    let json = serde_json::to_string(&profile).expect("serialize");
    assert_eq!(json, r#"{"id":"u1"}"#);
}

#[test]
fn generated_server_trait_implements_and_routes() {
    use generated::server_petstore;
    use server_petstore::Api;
    use server_petstore::CreatePetResponse;
    use server_petstore::DeletePetResponse;
    use server_petstore::Error;
    use server_petstore::GetPetResponse;
    use server_petstore::ListPetsResponse;
    use server_petstore::NewPet;
    use server_petstore::Pet;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn list_pets(&self) -> ListPetsResponse {
            return ListPetsResponse::Ok(Vec::new());
        }

        async fn create_pet(&self, body: NewPet) -> CreatePetResponse {
            return CreatePetResponse::Created(Pet {
                id: 1,
                name: body.name,
                tag: body.tag,
            });
        }

        async fn get_pet(&self, id: i64) -> GetPetResponse {
            return GetPetResponse::NotFound(Error {
                message: format!("no pet {id}"),
            });
        }

        async fn delete_pet(&self, _id: i64) -> DeletePetResponse {
            return DeletePetResponse::NoContent;
        }
    }

    // Building the router proves the native `async fn` trait, the response
    // enums' `IntoResponse`, and the handler wiring all type-check together.
    let _router: axum::Router = server_petstore::router(Service);
}

#[test]
fn generated_server_resolves_refs_and_import_mapping() {
    use generated::server_refs;
    use server_refs::Api;
    use server_refs::CreateWidgetResponse;
    use server_refs::ErrorBody;
    use server_refs::GetWidgetRawResponse;
    use server_refs::Widget;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn create_widget(&self, body: apimodel::CreateWidget) -> CreateWidgetResponse {
            if body.name.is_empty() {
                return CreateWidgetResponse::Unauthorized(ErrorBody {
                    message: "anonymous widgets are not allowed".to_owned(),
                });
            }
            return CreateWidgetResponse::Created(Widget {
                id: "w1".to_owned(),
                name: body.name,
            });
        }

        async fn get_widget_raw(&self, id: String) -> GetWidgetRawResponse {
            return GetWidgetRawResponse::Ok(serde_json::json!({ "id": id }));
        }
    }

    // The router builds only if the component-`$ref` response (`Unauthorized`),
    // the import-mapped body type (`crate::apimodel::CreateWidget`), and the
    // free-form `serde_json::Value` response all resolve and type-check.
    let _router: axum::Router = server_refs::router(Service);
}

#[test]
fn generated_server_accepts_query_params() {
    use generated::server_query_params;
    use server_query_params::Api;
    use server_query_params::Book;
    use server_query_params::ListBooksQuery;
    use server_query_params::ListBooksResponse;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn list_books(&self, query: ListBooksQuery) -> ListBooksResponse {
            let mut books = Vec::new();
            if query.available {
                let title = match query.author {
                    Some(author) => author,
                    None => "anon".to_owned(),
                };
                books.push(Book {
                    id: "b1".to_owned(),
                    title,
                });
            }
            return ListBooksResponse::Ok(books);
        }
    }

    // The query struct is read field-by-field above (required `available`,
    // optional `author`), and the router builds only if the generated
    // `axum_extra::extract::Query<ListBooksQuery>` extractor type-checks.
    let _router: axum::Router = server_query_params::router(Service);
}

#[test]
fn generated_server_accepts_header_params() {
    use generated::server_header_params;
    use server_header_params::Api;
    use server_header_params::GetWidgetsHeaders;
    use server_header_params::GetWidgetsResponse;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn get_widgets(&self, headers: GetWidgetsHeaders) -> GetWidgetsResponse {
            // Required headers are bare; optional ones are `Option<..>`. The
            // reserved `Authorization` header is absent from the struct.
            let _tenant: String = headers.x_tenant;
            let _request_id: String = headers.x_request_id;
            let _max_items: Option<i32> = headers.x_max_items;
            let _debug: Option<bool> = headers.x_debug;
            let _locale: Option<String> = headers.x_locale;
            return GetWidgetsResponse::Ok;
        }
    }

    // Building the router only type-checks if the generated `GetWidgetsHeaders`
    // satisfies axum's `FromRequestParts`, which is how the handler consumes it.
    let _router: axum::Router = server_header_params::router(Service);
}

#[test]
fn generated_server_supplies_status_for_default_and_range_responses() {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use generated::server_default_range_responses;
    use server_default_range_responses::Api;
    use server_default_range_responses::DeletePetResponse;
    use server_default_range_responses::Error;
    use server_default_range_responses::ListPetsResponse;
    use server_default_range_responses::Pet;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn list_pets(&self) -> ListPetsResponse {
            // Fixed `200` carries only the body; `5XX`/`default` carry the
            // concrete status the handler chooses.
            return ListPetsResponse::Status5xx(
                StatusCode::SERVICE_UNAVAILABLE,
                Error {
                    message: "down".to_owned(),
                },
            );
        }

        async fn delete_pet(&self, _id: i64) -> DeletePetResponse {
            return DeletePetResponse::Default(StatusCode::IM_A_TEAPOT);
        }
    }

    // The handler-supplied status code is the one the response actually renders.
    let ok = ListPetsResponse::Ok(Pet { id: 1 }).into_response();
    assert_eq!(ok.status(), StatusCode::OK);

    let range = ListPetsResponse::Status5xx(
        StatusCode::SERVICE_UNAVAILABLE,
        Error {
            message: "down".to_owned(),
        },
    )
    .into_response();
    assert_eq!(range.status(), StatusCode::SERVICE_UNAVAILABLE);

    let default = DeletePetResponse::Default(StatusCode::IM_A_TEAPOT).into_response();
    assert_eq!(default.status(), StatusCode::IM_A_TEAPOT);

    let _router: axum::Router = server_default_range_responses::router(Service);
}

#[test]
fn generated_server_accepts_cookie_params() {
    use generated::server_cookie_params;
    use server_cookie_params::Api;
    use server_cookie_params::GetWidgetsCookies;
    use server_cookie_params::GetWidgetsResponse;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn get_widgets(&self, cookies: GetWidgetsCookies) -> GetWidgetsResponse {
            // Required cookie is bare; optional is `Option<..>`.
            let _session: String = cookies.session;
            let _page_size: Option<i32> = cookies.page_size;
            return GetWidgetsResponse::Ok;
        }
    }

    // Building the router only type-checks if the generated `GetWidgetsCookies`
    // satisfies axum's `FromRequestParts`, which is how the handler consumes it.
    let _router: axum::Router = server_cookie_params::router(Service);
}

#[test]
fn generated_server_resolves_component_param_refs() {
    use generated::server_component_param_ref;
    use server_component_param_ref::Api;
    use server_component_param_ref::GetWidgetCookies;
    use server_component_param_ref::GetWidgetHeaders;
    use server_component_param_ref::GetWidgetQuery;
    use server_component_param_ref::GetWidgetResponse;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn get_widget(
            &self,
            id: String,
            query: GetWidgetQuery,
            headers: GetWidgetHeaders,
            cookies: GetWidgetCookies,
        ) -> GetWidgetResponse {
            // All four parameter locations resolved from component `$ref`s.
            let _id: String = id;
            let _verbose: Option<bool> = query.verbose;
            let _request_id: String = headers.x_request_id;
            let _session: Option<String> = cookies.session;
            return GetWidgetResponse::Ok;
        }
    }

    let _router: axum::Router = server_component_param_ref::router(Service);
}

#[test]
fn generated_server_resolves_component_body_ref() {
    use generated::server_component_body_ref;
    use server_component_body_ref::Api;
    use server_component_body_ref::CreateWidgetResponse;
    use server_component_body_ref::NewWidget;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn create_widget(&self, body: NewWidget) -> CreateWidgetResponse {
            let _name: String = body.name;
            return CreateWidgetResponse::Created;
        }
    }

    let _router: axum::Router = server_component_body_ref::router(Service);
}

#[test]
fn generated_server_resolves_cross_file_param_ref() {
    use generated::server_xfile_refs;
    use server_xfile_refs::Api;
    use server_xfile_refs::CreateThingResponse;
    use server_xfile_refs::ListThingsQuery;
    use server_xfile_refs::ListThingsResponse;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn list_things(&self, query: ListThingsQuery) -> ListThingsResponse {
            // The `pageSize` query param was resolved from `schemas/shared.yaml`
            // and lowered to its scalar type (`i32`).
            let _page_size: Option<i32> = query.page_size;
            return ListThingsResponse::Ok;
        }

        async fn create_thing(&self, body: apimodel::NewThing) -> CreateThingResponse {
            // The request body `$ref` targets `schemas/shared.yaml`, whose inner
            // `$ref` to `NewThing` is rewritten to the import-mapped module
            // (`crate::apimodel::NewThing`). The cross-file `404` response ref
            // resolves to a `NotFound` variant.
            let _name: String = body.name;
            return CreateThingResponse::NotFound;
        }
    }

    let _router: axum::Router = server_xfile_refs::router(Service);
}

/// Two runs of the generator compose one crate: `compose_shared.yaml` gives the
/// models, and `multi_spec_compose.yaml` gives the operations that carry them.
///
/// No stand-in stands between the two. Both sides are generated, so the test
/// fails to compile if the runs disagree on a name. `Parcel` carries an
/// `x-rust-name` for that reason: the models run emits `Shipment`, and the
/// operations run must reach the same name through the `import-mapping`.
#[test]
fn two_generator_runs_compose_one_crate() {
    use generated::compose_shared;
    use generated::multi_spec_compose;
    use multi_spec_compose::AcceptParcelResponse;
    use multi_spec_compose::Api;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn accept_parcel(&self, body: compose_shared::Shipment) -> AcceptParcelResponse {
            return AcceptParcelResponse::Created(compose_shared::ParcelReceipt {
                id: body.id,
                accepted: true,
            });
        }
    }

    let _router: axum::Router = multi_spec_compose::router(Service);

    // The body the handler takes is the type the other run wrote, so a payload
    // the models crate reads is a payload the operations crate accepts.
    let json = r#"{"id":"p-1","weight_kg":2.5}"#;
    let parcel: compose_shared::Shipment = serde_json::from_str(json).expect("read a parcel");
    assert_eq!(parcel.id, "p-1");
    assert_eq!(parcel.weight_kg, 2.5_f64);
}

#[test]
fn generated_server_writes_response_headers() {
    use axum::response::IntoResponse;
    use generated::server_response_headers;
    use server_response_headers::Api;
    use server_response_headers::GetWidgetsResponse;

    const SAMPLE_RATE_LIMIT_REMAINING: i32 = 42;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn get_widgets(&self) -> GetWidgetsResponse {
            return GetWidgetsResponse::Ok {
                body: vec!["w1".to_owned()],
                x_request_id: "abc-123".to_owned(),
                x_rate_limit_remaining: Some(SAMPLE_RATE_LIMIT_REMAINING),
            };
        }
    }

    // The `Ok` variant renders its headers into the response.
    let response = GetWidgetsResponse::Ok {
        body: vec!["w1".to_owned()],
        x_request_id: "abc-123".to_owned(),
        x_rate_limit_remaining: Some(SAMPLE_RATE_LIMIT_REMAINING),
    }
    .into_response();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .expect("missing x-request-id header"),
        "abc-123"
    );
    assert_eq!(
        response
            .headers()
            .get("x-ratelimit-remaining")
            .expect("missing x-ratelimit-remaining header")
            .to_str()
            .expect("header value is valid UTF-8"),
        SAMPLE_RATE_LIMIT_REMAINING.to_string()
    );

    // An unset optional header is absent.
    let response = GetWidgetsResponse::Ok {
        body: Vec::new(),
        x_request_id: "abc-123".to_owned(),
        x_rate_limit_remaining: None,
    }
    .into_response();
    assert!(response.headers().get("x-ratelimit-remaining").is_none());

    // The dynamic `Default` variant uses the handler-supplied status.
    let response = GetWidgetsResponse::Default {
        status: axum::http::StatusCode::BAD_GATEWAY,
        body: "boom".to_owned(),
        x_request_id: "abc-123".to_owned(),
    }
    .into_response();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .expect("missing x-request-id header"),
        "abc-123"
    );

    // Building the router proves the trait + handler wiring type-check.
    let _router: axum::Router = server_response_headers::router(Service);
}

#[test]
fn generated_server_handles_text_body() {
    use axum::response::IntoResponse;
    use generated::server_text_body;
    use server_text_body::Api;
    use server_text_body::EchoResponse;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn echo(&self, body: String) -> EchoResponse {
            return EchoResponse::Ok(body);
        }
    }

    // The `text/plain` request body extracts as a bare `String` (no `axum::Json`)
    // and the response renders the `String` directly.
    let response = EchoResponse::Ok("hi".to_owned()).into_response();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let _router: axum::Router = server_text_body::router(Service);
}

#[test]
fn generated_server_handles_form_body() {
    use axum::response::IntoResponse;
    use generated::server_form_body;
    use server_form_body::Api;
    use server_form_body::Credentials;
    use server_form_body::LoginResponse;
    use server_form_body::Session;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn login(&self, body: Credentials) -> LoginResponse {
            let _u: String = body.username;
            let _p: String = body.password;
            return LoginResponse::Ok(Session { token: "t".to_owned() });
        }
    }

    // The response renders the `Session` struct as an `axum::Form` body; the
    // router builds only if both the `axum::Form<Credentials>` request extractor
    // and the form response wrapper type-check (both derive Serialize/Deserialize).
    let response = LoginResponse::Ok(Session { token: "t".to_owned() }).into_response();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let _router: axum::Router = server_form_body::router(Service);
}

#[test]
fn generated_server_handles_multipart_body() {
    use generated::server_multipart_body;
    use server_multipart_body::Api;
    use server_multipart_body::UploadMultipart;
    use server_multipart_body::UploadResponse;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn upload(&self, body: UploadMultipart) -> UploadResponse {
            // File fields decode to `Vec<u8>`, scalars are parsed from the part
            // text (required ones bare, optional ones `Option<..>`).
            let _file: Vec<u8> = body.file;
            let _description: String = body.description;
            let _note: Option<String> = body.note;
            let _attempts: i64 = body.attempts;
            let _priority: Option<String> = body.priority;
            return UploadResponse::NoContent;
        }
    }

    // Building the router only type-checks if the generated `UploadMultipart`
    // struct satisfies axum's `FromRequest` (driving `axum::extract::Multipart`),
    // which is how the handler consumes the `multipart/form-data` body.
    let _router: axum::Router = server_multipart_body::router(Service);
}

/// Read a full HTTP/1.1 request (head and body) from a mock-server connection,
/// returning it as a lossy UTF-8 string so tests can assert on the request line,
/// headers, and body. Handles both `Content-Length` and `Transfer-Encoding:
/// chunked` framing, since `reqwest` can stream some bodies (for example, multipart)
/// without declaring a length up front.
#[allow(
    clippy::expect_used,
    reason = "test-support helper: panicking is the correct failure mode when a mock request can't be read"
)]
fn read_request(stream: &mut std::net::TcpStream) -> String {
    use std::io::BufRead;
    use std::io::Read;

    let mut reader = std::io::BufReader::new(stream);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).expect("read request line");
        if read == 0 {
            break;
        }
        let blank = line == "\r\n";
        head.push_str(&line);
        if blank {
            break;
        }
    }
    let transfer_encoding = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("transfer-encoding") {
            return Some(value.trim().to_ascii_lowercase());
        }
        return None;
    });
    let mut body = Vec::new();
    if matches!(transfer_encoding.as_deref(), Some(encoding) if encoding.contains("chunked")) {
        loop {
            let mut size_line = String::new();
            reader.read_line(&mut size_line).expect("read chunk size");
            let size_str = size_line.trim_end_matches("\r\n").split(';').next().unwrap_or("");
            let size = usize::from_str_radix(size_str.trim(), 16).expect("parse chunk size");
            if size == 0 {
                let mut crlf = [0_u8; 2];
                reader.read_exact(&mut crlf).expect("read final chunk crlf");
                break;
            }
            let mut chunk = vec![0_u8; size];
            reader.read_exact(&mut chunk).expect("read chunk body");
            body.extend_from_slice(&chunk);
            let mut crlf = [0_u8; 2];
            reader.read_exact(&mut crlf).expect("read chunk crlf");
        }
    } else {
        let content_length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.trim().eq_ignore_ascii_case("content-length") {
                    return value.trim().parse::<usize>().ok();
                }
                return None;
            })
            .unwrap_or(0);
        body.resize(content_length, 0_u8);
        if content_length > 0 {
            reader.read_exact(&mut body).expect("read request body");
        }
    }
    return format!("{head}{}", String::from_utf8_lossy(&body));
}

/// Build a canned HTTP/1.1 response with `Connection: close` (so the blocking
/// client opens a fresh socket per call), an optional `Content-Type`, any extra
/// headers, and a body whose length sets `Content-Length`.
fn response(status: &str, content_type: Option<&str>, extra: &[(&str, &str)], body: &str) -> String {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {len}\r\n",
        len = body.len(),
    );
    if let Some(content_type) = content_type {
        out.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    for (name, value) in extra {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(body);
    return out;
}

/// Drive the generated blocking client against a tiny canned HTTP server to
/// prove its request building and response decoding behave at runtime (not just
/// compile). Each response sets `Connection: close` so the blocking client opens
/// a fresh socket per call, letting the mock accept them in order; the mock also
/// captures each raw request so the generated request-building can be asserted.
///
/// Coverage: query building, JSON request bodies + request headers, JSON
/// responses, a bodyless `404`, a `5XX` range dispatch carrying the status and a
/// decoded body, form request bodies, cookie headers, and a `text/plain` request
/// whose response carries a decoded body plus a parsed response header.
#[test]
fn generated_client_drives_requests_and_decodes_responses() {
    use std::net::TcpListener;

    use client_widgets::AddNoteResponse;
    use client_widgets::Client;
    use client_widgets::CreateWidgetHeaders;
    use client_widgets::CreateWidgetResponse;
    use client_widgets::DeleteWidgetCookies;
    use client_widgets::DeleteWidgetResponse;
    use client_widgets::GetWidgetResponse;
    use client_widgets::ListWidgetsQuery;
    use client_widgets::ListWidgetsResponse;
    use client_widgets::NewWidget;
    use client_widgets::ReplaceWidgetResponse;
    use generated::client_widgets;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("mock server addr");
    let base_url = format!("http://{addr}");

    let responses = vec![
        response(
            "200 OK",
            Some("application/json"),
            &[],
            r#"[{"id":"w9","name":"Chair"}]"#,
        ),
        response(
            "201 Created",
            Some("application/json"),
            &[("Location", "/widgets/w10")],
            r#"{"id":"w10","name":"Gizmo"}"#,
        ),
        response(
            "200 OK",
            Some("application/json"),
            &[],
            r#"{"id":"w1","name":"Widget One"}"#,
        ),
        response("404 Not Found", None, &[], ""),
        response(
            "503 Service Unavailable",
            Some("application/json"),
            &[],
            r#"{"code":"upstream","message":"down"}"#,
        ),
        response(
            "200 OK",
            Some("application/json"),
            &[],
            r#"{"id":"w1","name":"Renamed"}"#,
        ),
        response("204 No Content", None, &[], ""),
        response("201 Created", Some("text/plain"), &[("X-Note-Id", "note-7")], "stored"),
    ];

    let server = std::thread::spawn(move || {
        use std::io::Write;

        let mut received = Vec::with_capacity(responses.len());
        for response in &responses {
            let (mut stream, _) = listener.accept().expect("accept mock connection");
            received.push(read_request(&mut stream));
            stream.write_all(response.as_bytes()).expect("write mock response");
            stream.flush().expect("flush mock response");
        }
        return received;
    });

    let client = Client::new(base_url).expect("build client");

    let query = ListWidgetsQuery {
        q: Some("chair".to_owned()),
        tags: Some(vec!["a".to_owned(), "b".to_owned()]),
        limit: 10,
        region: "eu".to_owned(),
    };
    match client.list_widgets(query).expect("list call succeeds") {
        ListWidgetsResponse::Ok(widgets) => {
            assert_eq!(widgets.len(), 1);
            assert_eq!(widgets[0].id, "w9");
        }
    }

    let headers = CreateWidgetHeaders {
        idempotency_key: "key-1".to_owned(),
        x_trace_id: Some("trace-1".to_owned()),
    };
    let new_widget = NewWidget {
        name: "Gizmo".to_owned(),
        tags: Some(vec!["red".to_owned()]),
    };
    match client.create_widget(headers, new_widget).expect("create call succeeds") {
        CreateWidgetResponse::Created { body, location } => {
            assert_eq!(body.id, "w10");
            assert_eq!(location.as_deref(), Some("/widgets/w10"));
        }
        _ => panic!("expected CreateWidgetResponse::Created for a 201 response"),
    }

    match client.get_widget("w1".to_owned()).expect("200 call succeeds") {
        GetWidgetResponse::Ok(widget) => {
            assert_eq!(widget.id, "w1");
            assert_eq!(widget.name, "Widget One");
        }
        _ => panic!("expected GetWidgetResponse::Ok for a 200 response"),
    }

    match client.get_widget("missing".to_owned()).expect("404 call succeeds") {
        GetWidgetResponse::NotFound => {}
        _ => panic!("expected GetWidgetResponse::NotFound for a 404 response"),
    }

    match client.get_widget("boom".to_owned()).expect("503 call succeeds") {
        GetWidgetResponse::Status5xx(status, error) => {
            assert_eq!(status.as_u16(), 503);
            assert_eq!(error.code, "upstream");
        }
        _ => panic!("expected GetWidgetResponse::Status5xx for a 503 response"),
    }

    let replacement = NewWidget {
        name: "Renamed".to_owned(),
        tags: None,
    };
    match client
        .replace_widget("w1".to_owned(), replacement)
        .expect("replace call succeeds")
    {
        ReplaceWidgetResponse::Ok(widget) => assert_eq!(widget.name, "Renamed"),
    }

    let cookies = DeleteWidgetCookies {
        session: "abc123".to_owned(),
    };
    match client
        .delete_widget("w1".to_owned(), cookies)
        .expect("delete call succeeds")
    {
        DeleteWidgetResponse::NoContent => {}
    }

    match client
        .add_note("w1".to_owned(), "hello note".to_owned())
        .expect("note call succeeds")
    {
        AddNoteResponse::Created { body, x_note_id } => {
            assert_eq!(body, "stored");
            assert_eq!(x_note_id, "note-7");
        }
    }

    let received = server.join().expect("mock server thread");

    let list_request = &received[0];
    assert!(list_request.starts_with("GET /widgets?"), "list path: {list_request}");
    for pair in ["q=chair", "tags=a", "tags=b", "limit=10", "region=eu"] {
        assert!(
            list_request.contains(pair),
            "list query missing `{pair}`: {list_request}"
        );
    }

    let create_request = &received[1];
    let create_lower = create_request.to_lowercase();
    assert!(
        create_request.starts_with("POST /widgets "),
        "create path: {create_request}"
    );
    assert!(create_lower.contains("idempotency-key: key-1"));
    assert!(create_lower.contains("x-trace-id: trace-1"));
    assert!(create_lower.contains("content-type: application/json"));
    assert!(
        create_request.contains(r#""name":"Gizmo""#),
        "create body: {create_request}"
    );

    let replace_request = &received[5];
    assert!(
        replace_request.starts_with("PUT /widgets/w1 "),
        "replace path: {replace_request}"
    );
    assert!(
        replace_request
            .to_lowercase()
            .contains("content-type: application/x-www-form-urlencoded")
    );
    assert!(
        replace_request.contains("name=Renamed"),
        "replace body: {replace_request}"
    );

    let delete_request = &received[6];
    assert!(
        delete_request.starts_with("DELETE /widgets/w1 "),
        "delete path: {delete_request}"
    );
    assert!(delete_request.to_lowercase().contains("cookie: session=abc123"));

    let note_request = &received[7];
    assert!(
        note_request.starts_with("POST /widgets/w1/notes "),
        "note path: {note_request}"
    );
    assert!(note_request.to_lowercase().contains("content-type: text/plain"));
    assert!(note_request.contains("hello note"), "note body: {note_request}");
}

/// Drive the generated clients for the multi-content body shapes against a canned
/// HTTP server, proving the request encoders and response decoders added for
/// multipart requests, negotiated request/response bodies, and form responses
/// work on the wire.
///
/// Coverage: a multipart request (file + optional text + scalar parts), a
/// negotiated request sent as JSON and as form, a negotiated response decoded by
/// `Content-Type` (JSON and text), and a form-urlencoded response body.
#[test]
fn generated_client_encodes_and_decodes_body_shapes() {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("mock server addr");
    let base_url = format!("http://{addr}");

    let responses = vec![
        response("204 No Content", None, &[], ""),
        response("204 No Content", None, &[], ""),
        response("204 No Content", None, &[], ""),
        response("200 OK", Some("application/json"), &[], r#"{"id":"r1"}"#),
        response("200 OK", Some("text/plain"), &[], "plain report"),
        response("200 OK", Some("application/x-www-form-urlencoded"), &[], "name=Ada"),
    ];

    let server = std::thread::spawn(move || {
        use std::io::Write;

        let mut received = Vec::with_capacity(responses.len());
        for response in &responses {
            let (mut stream, _) = listener.accept().expect("accept mock connection");
            received.push(read_request(&mut stream));
            stream.write_all(response.as_bytes()).expect("write mock response");
            stream.flush().expect("flush mock response");
        }
        return received;
    });

    {
        use generated::client_multipart_request::Client;
        use generated::client_multipart_request::UploadMultipart;
        use generated::client_multipart_request::UploadResponse;

        let client = Client::new(&base_url).expect("build multipart client");
        let body = UploadMultipart {
            image: b"\x89PNG".to_vec(),
            caption: Some("a cat".to_owned()),
            attempts: 3,
        };
        match client.upload(body).expect("multipart upload succeeds") {
            UploadResponse::NoContent => {}
        }
    }

    {
        use generated::client_negotiated_request::Client;
        use generated::client_negotiated_request::CreateThingRequestBody;
        use generated::client_negotiated_request::CreateThingResponse;
        use generated::client_negotiated_request::Thing;

        let client = Client::new(&base_url).expect("build negotiated-request client");
        let json_body = CreateThingRequestBody::Json(Thing {
            name: "as-json".to_owned(),
        });
        match client.create_thing(json_body).expect("json create succeeds") {
            CreateThingResponse::NoContent => {}
        }
        let form_body = CreateThingRequestBody::Form(Thing {
            name: "as-form".to_owned(),
        });
        match client.create_thing(form_body).expect("form create succeeds") {
            CreateThingResponse::NoContent => {}
        }
    }

    {
        use generated::client_negotiated_response::Client;
        use generated::client_negotiated_response::GetReportResponse;
        use generated::client_negotiated_response::GetReportResponseOkBody;

        let client = Client::new(&base_url).expect("build negotiated-response client");
        match client.get_report().expect("json report succeeds") {
            GetReportResponse::Ok(GetReportResponseOkBody::Json(report)) => {
                assert_eq!(report.id, "r1");
            }
            _ => panic!("expected a JSON-decoded report for an application/json response"),
        }
        match client.get_report().expect("text report succeeds") {
            GetReportResponse::Ok(GetReportResponseOkBody::Text(text)) => {
                assert_eq!(text, "plain report");
            }
            _ => panic!("expected a text-decoded report for a text/plain response"),
        }
    }

    {
        use generated::client_form_response::Client;
        use generated::client_form_response::GetFormResponse;

        let client = Client::new(&base_url).expect("build form-response client");
        match client.get_form().expect("form response succeeds") {
            GetFormResponse::Ok(form) => {
                assert_eq!(form.name, "Ada");
            }
        }
    }

    let received = server.join().expect("mock server thread");

    let upload_request = &received[0];
    assert!(
        upload_request.starts_with("POST /upload "),
        "upload path: {upload_request}"
    );
    assert!(
        upload_request
            .to_lowercase()
            .contains("content-type: multipart/form-data"),
        "upload content type: {upload_request}"
    );
    assert!(
        upload_request.contains("name=\"image\""),
        "upload parts: {upload_request}"
    );
    assert!(
        upload_request.contains("name=\"caption\""),
        "upload parts: {upload_request}"
    );
    assert!(
        upload_request.contains("name=\"attempts\""),
        "upload parts: {upload_request}"
    );

    let json_request = &received[1];
    assert!(json_request.to_lowercase().contains("content-type: application/json"));
    assert!(
        json_request.contains(r#""name":"as-json""#),
        "json body: {json_request}"
    );

    let form_request = &received[2];
    assert!(
        form_request
            .to_lowercase()
            .contains("content-type: application/x-www-form-urlencoded")
    );
    assert!(form_request.contains("name=as-form"), "form body: {form_request}");
}

/// Drive the generated auth client against a canned HTTP server to prove each
/// security scheme places its credential on the wire, that an operation with
/// `security: []` sends none, and that an unset credential is simply omitted.
///
/// Coverage: global bearer auth, an unauthenticated operation, a per-operation
/// HTTP basic override, and API-key credentials carried in a header, a query
/// parameter, and a cookie.
#[test]
fn generated_client_applies_security_credentials() {
    use std::net::TcpListener;

    use client_auth::Client;
    use client_auth::GetAdminResponse;
    use client_auth::GetProfileResponse;
    use client_auth::GetPublicResponse;
    use client_auth::GetReportsResponse;
    use client_auth::GetSessionResponse;
    use client_auth::SearchQuery;
    use client_auth::SearchResponse;
    use generated::client_auth;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("mock server addr");
    let base_url = format!("http://{addr}");

    let server = std::thread::spawn(move || {
        use std::io::Write;

        /// One request per security scheme this test exercises: global bearer,
        /// unauthenticated, per-operation basic override, and header/query/cookie
        /// API keys.
        const REQUEST_COUNT: usize = 6;

        let mut received = Vec::with_capacity(REQUEST_COUNT);
        for _ in 0..REQUEST_COUNT {
            let (mut stream, _) = listener.accept().expect("accept mock connection");
            received.push(read_request(&mut stream));
            let body = r#"{"text":"ok"}"#;
            stream
                .write_all(response("200 OK", Some("application/json"), &[], body).as_bytes())
                .expect("write mock response");
            stream.flush().expect("flush mock response");
        }
        return received;
    });

    let client = Client::new(base_url)
        .expect("build client")
        .with_bearer_auth("tok-123")
        .with_basic_auth("user", "pass")
        .with_api_key_header("header-key")
        .with_api_key_query("query-key")
        .with_api_key_cookie("cookie-key");

    match client.get_profile().expect("profile call succeeds") {
        GetProfileResponse::Ok(message) => assert_eq!(message.text, "ok"),
    }
    match client.get_public().expect("public call succeeds") {
        GetPublicResponse::Ok(message) => assert_eq!(message.text, "ok"),
    }
    match client.get_admin().expect("admin call succeeds") {
        GetAdminResponse::Ok(message) => assert_eq!(message.text, "ok"),
    }
    match client.get_reports().expect("reports call succeeds") {
        GetReportsResponse::Ok(message) => assert_eq!(message.text, "ok"),
    }
    let query = SearchQuery { q: "chair".to_owned() };
    match client.search(query).expect("search call succeeds") {
        SearchResponse::Ok(message) => assert_eq!(message.text, "ok"),
    }
    match client.get_session().expect("session call succeeds") {
        GetSessionResponse::Ok(message) => assert_eq!(message.text, "ok"),
    }

    let received = server.join().expect("mock server thread");

    let profile_request = received[0].to_lowercase();
    assert!(
        received[0].starts_with("GET /profile "),
        "profile path: {}",
        received[0]
    );
    assert!(
        profile_request.contains("authorization: bearer tok-123"),
        "profile auth: {}",
        received[0]
    );

    let public_request = received[1].to_lowercase();
    assert!(received[1].starts_with("GET /public "), "public path: {}", received[1]);
    assert!(
        !public_request.contains("authorization:"),
        "public request must carry no auth: {}",
        received[1]
    );

    let admin_request = received[2].to_lowercase();
    assert!(
        admin_request.contains("authorization: basic ") && received[2].contains("Basic dXNlcjpwYXNz"),
        "admin basic auth: {}",
        received[2]
    );

    let reports_request = received[3].to_lowercase();
    assert!(
        reports_request.contains("x-api-key: header-key"),
        "reports header key: {}",
        received[3]
    );

    assert!(received[4].starts_with("GET /search?"), "search path: {}", received[4]);
    assert!(
        received[4].contains("q=chair") && received[4].contains("api_key=query-key"),
        "search query key: {}",
        received[4]
    );

    let session_request = received[5].to_lowercase();
    assert!(
        session_request.contains("cookie: session=cookie-key"),
        "session cookie key: {}",
        received[5]
    );
}
