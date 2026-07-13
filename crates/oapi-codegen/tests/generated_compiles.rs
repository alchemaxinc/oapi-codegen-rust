//! Compile-checks every *supported* generated output and exercises a few
//! representative types at runtime.
//!
//! Each module `include!`s a generated file so the test crate fails to build if
//! any emitted code stops compiling against its real dependencies (serde,
//! chrono, uuid, serde_json, axum). The module set is kept in lock-step with the
//! coverage matrix by `generated_outputs_are_compile_checked` in
//! `tests/coverage.rs`.
//!
//! The generated modules live under `mod generated`, which carries the only lint
//! exceptions in this file: emitted code is ordinary idiomatic Rust (tail
//! expressions, plus types these tests never construct), and the workspace's
//! `implicit_return`/`dead_code` rules are about first-party source, not
//! generated output. The handwritten tests below are linted normally.

#[allow(dead_code, clippy::implicit_return, clippy::collapsible_if)]
mod generated {
    pub mod allof_merge {
        include!("generated/allof_merge.rs");
    }
    pub mod anyof_untagged {
        include!("generated/anyof_untagged.rs");
    }
    pub mod array_types {
        include!("generated/array_types.rs");
    }
    pub mod ext_vendor_extensions {
        include!("generated/ext_vendor_extensions.rs");
    }
    pub mod ext_x_rust_type {
        include!("generated/ext_x_rust_type.rs");
    }
    pub mod freeform_any {
        include!("generated/freeform_any.rs");
    }
    pub mod integer_formats {
        include!("generated/integer_formats.rs");
    }
    pub mod map_alias {
        include!("generated/map_alias.rs");
    }
    pub mod metadata_docs {
        include!("generated/metadata_docs.rs");
    }
    pub mod nullable {
        include!("generated/nullable.rs");
    }
    pub mod number_formats {
        include!("generated/number_formats.rs");
    }
    pub mod object_additional_properties {
        include!("generated/object_additional_properties.rs");
    }
    pub mod object_nested_inline {
        include!("generated/object_nested_inline.rs");
    }
    pub mod object_optional_required {
        include!("generated/object_optional_required.rs");
    }
    pub mod oneof_discriminator {
        include!("generated/oneof_discriminator.rs");
    }
    pub mod oneof_untagged {
        include!("generated/oneof_untagged.rs");
    }
    pub mod primitive_scalars {
        include!("generated/primitive_scalars.rs");
    }
    pub mod ref_local {
        include!("generated/ref_local.rs");
    }
    pub mod string_enum {
        include!("generated/string_enum.rs");
    }
    pub mod string_formats {
        include!("generated/string_formats.rs");
    }
    pub mod server_petstore {
        include!("generated/server_petstore.rs");
    }
    pub mod server_refs {
        include!("generated/server_refs.rs");
    }
    pub mod server_query_params {
        include!("generated/server_query_params.rs");
    }
    pub mod server_header_params {
        include!("generated/server_header_params.rs");
    }
    pub mod server_default_range_responses {
        include!("generated/server_default_range_responses.rs");
    }
    pub mod server_cookie_params {
        include!("generated/server_cookie_params.rs");
    }
    pub mod server_component_param_ref {
        include!("generated/server_component_param_ref.rs");
    }
    pub mod server_component_body_ref {
        include!("generated/server_component_body_ref.rs");
    }
    pub mod server_component_param_ref_pet {
        include!("generated/server_component_param_ref_pet.rs");
    }
    pub mod server_xfile_refs {
        include!("generated/server_xfile_refs.rs");
    }
    pub mod server_response_headers {
        include!("generated/server_response_headers.rs");
    }
    pub mod server_text_body {
        include!("generated/server_text_body.rs");
    }
    pub mod server_form_body {
        include!("generated/server_form_body.rs");
    }
    pub mod server_multipart_body {
        include!("generated/server_multipart_body.rs");
    }
    pub mod server_json_charset {
        include!("generated/server_json_charset.rs");
    }
    pub mod server_multi_content_request {
        include!("generated/server_multi_content_request.rs");
    }
    pub mod server_multi_content_response {
        include!("generated/server_multi_content_response.rs");
    }
    pub mod server_prune {
        include!("generated/server_prune.rs");
    }
    pub mod server_filtering {
        include!("generated/server_filtering.rs");
    }
    pub mod server_urls {
        include!("generated/server_urls.rs");
    }
    pub mod client_widgets {
        include!("generated/client_widgets.rs");
    }
    pub mod client_auth {
        include!("generated/client_auth.rs");
    }
}

/// Stand-in for the models crate the `server_refs` fixture's `import-mapping`
/// points its cross-file `$ref` bodies at (`crate::apimodel`). Real projects
/// generate this module from the referenced schema file; here a minimal struct
/// proves the emitted `crate::apimodel::CreateWidget` path resolves and that the
/// generated handler can decode it as a JSON body.
#[allow(dead_code)]
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

#[test]
fn generated_server_writes_response_headers() {
    use axum::response::IntoResponse;
    use generated::server_response_headers;
    use server_response_headers::Api;
    use server_response_headers::GetWidgetsResponse;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn get_widgets(&self) -> GetWidgetsResponse {
            return GetWidgetsResponse::Ok {
                body: vec!["w1".to_owned()],
                x_request_id: "abc-123".to_owned(),
                x_rate_limit_remaining: Some(42),
            };
        }
    }

    // The `Ok` variant renders its headers into the response.
    let response = GetWidgetsResponse::Ok {
        body: vec!["w1".to_owned()],
        x_request_id: "abc-123".to_owned(),
        x_rate_limit_remaining: Some(42),
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
            .expect("missing x-ratelimit-remaining header"),
        "42"
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
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body).expect("read request body");
        }
        return format!("{head}{}", String::from_utf8_lossy(&body));
    }

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
            assert_eq!(x_note_id.as_deref(), Some("note-7"));
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

    fn read_request(stream: &mut std::net::TcpStream) -> String {
        use std::io::BufRead;

        let mut reader = std::io::BufReader::new(stream);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            let read = reader.read_line(&mut line).expect("read request line");
            if read == 0 || line == "\r\n" {
                break;
            }
            head.push_str(&line);
        }
        return head;
    }

    fn response(body: &str) -> String {
        return format!(
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {len}\r\n\r\n{body}",
            len = body.len(),
        );
    }

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("mock server addr");
    let base_url = format!("http://{addr}");

    let server = std::thread::spawn(move || {
        use std::io::Write;

        let mut received = Vec::with_capacity(6);
        for _ in 0..6 {
            let (mut stream, _) = listener.accept().expect("accept mock connection");
            received.push(read_request(&mut stream));
            let body = r#"{"text":"ok"}"#;
            stream
                .write_all(response(body).as_bytes())
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
