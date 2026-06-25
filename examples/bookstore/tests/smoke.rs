//! Smoke test for the composed example: a hand-written `Api` implementation
//! backed by the generated model types, wired into the generated axum router.
//!
//! This proves the whole pipeline composes — the server's cross-file `$ref`s
//! resolve to the generated `apimodel` modules, the trait's native async methods
//! are implementable, and the `Router` builder accepts the implementation. We
//! deliberately avoid pulling in an async runtime: building the router exercises
//! all of the generated wiring, and the `impl Api` block proves every operation
//! is implementable in terms of the imported model types.

use bookstore_example::apimodel::catalog::Book;
use bookstore_example::apimodel::catalog::NewBook;
use bookstore_example::apimodel::common::ErrorResponse;
use bookstore_example::restapi::Api;
use bookstore_example::restapi::CreateBookResponse;
use bookstore_example::restapi::GetBookResponse;
use bookstore_example::restapi::GetHealthResponse;

#[derive(Clone)]
struct Service;

impl Api for Service {
    async fn create_book(&self, body: NewBook) -> CreateBookResponse {
        if body.title.is_empty() {
            return CreateBookResponse::BadRequest(ErrorResponse {
                code: "empty_title".to_owned(),
                message: "title must not be empty".to_owned(),
            });
        }

        return CreateBookResponse::Created(Book {
            id: "book-1".to_owned(),
            title: body.title,
            author: body.author,
            price_cents: body.price_cents,
            tags: body.tags,
        });
    }

    async fn get_book(&self, id: String) -> GetBookResponse {
        if id == "book-1" {
            return GetBookResponse::Ok(Book {
                id,
                title: "The Rust Programming Language".to_owned(),
                author: "Klabnik & Nichols".to_owned(),
                price_cents: 3999,
                tags: Some(vec!["rust".to_owned()]),
            });
        }

        return GetBookResponse::NotFound;
    }

    async fn get_health(&self) -> GetHealthResponse {
        return GetHealthResponse::Ok(serde_json::json!({ "status": "ok" }));
    }
}

#[test]
fn router_builds_from_a_real_api_implementation() {
    let _router = bookstore_example::restapi::router(Service);
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
