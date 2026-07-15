//! Smoke test for the composed example: the shared in-memory [`Service`]
//! (see `src/service.rs`) implements the generated `Api` trait, and we wire it
//! into the generated axum router.
//!
//! This proves the whole pipeline composes — the server's cross-file `$ref`s
//! resolve to the generated `apimodel` modules, the trait's native async methods
//! are implementable, and the `Router` builder accepts the implementation. We
//! deliberately avoid pulling in an async runtime here: building the router
//! exercises all of the generated wiring without invoking any handler.

use bookstore_example::Service;
use bookstore_example::apimodel::catalog::Book;

#[test]
fn router_builds_from_a_real_api_implementation() {
    let _router = bookstore_example::restapi::router(Service::new());
}

#[test]
fn generated_models_round_trip_through_serde() {
    let book = Book {
        id: "book-1".to_owned(),
        title: "Programming Rust".to_owned(),
        author: "Blandy, Orendorff & Tindall".to_owned(),
        price_cents: 5999,
        tags: None,
    };

    let json = serde_json::to_string(&book).expect("serialize Book");
    let parsed: Book = serde_json::from_str(&json).expect("deserialize Book");
    assert_eq!(book, parsed);
}

#[test]
fn generated_server_urls_resolve() {
    use bookstore_example::restapi::SERVER_URL_PRODUCTION;
    use bookstore_example::restapi::ServerUrlRegionalRegion;
    use bookstore_example::restapi::server_url_regional;

    assert_eq!(SERVER_URL_PRODUCTION, "https://api.bookstore.example.com/v1");

    let url = server_url_regional(ServerUrlRegionalRegion::Eu).expect("substitutes the region variable");
    assert_eq!(url, "https://eu.api.bookstore.example.com/v1");

    // The `Default` impl points at the OpenAPI-declared default (`us`).
    let default = server_url_regional(ServerUrlRegionalRegion::default()).expect("default region resolves");
    assert_eq!(default, "https://us.api.bookstore.example.com/v1");
}
