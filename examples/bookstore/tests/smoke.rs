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
use bookstore_example::apimodel::catalog::Review;
use bookstore_example::apimodel::common::ErrorResponse;
use bookstore_example::restapi::Api;
use bookstore_example::restapi::CreateBookHeaders;
use bookstore_example::restapi::CreateBookResponse;
use bookstore_example::restapi::GetBookResponse;
use bookstore_example::restapi::GetHealthResponse;
use bookstore_example::restapi::ListBooksQuery;
use bookstore_example::restapi::ListBooksResponse;
use bookstore_example::restapi::SubmitReviewRequestBody;
use bookstore_example::restapi::SubmitReviewResponse;
use bookstore_example::restapi::SubmitReviewResponseCreatedBody;
use bookstore_example::restapi::UploadBookCoverMultipart;
use bookstore_example::restapi::UploadBookCoverResponse;

#[derive(Clone)]
struct Service;

impl Api for Service {
    async fn list_books(&self, query: ListBooksQuery) -> ListBooksResponse {
        let author = match query.author {
            Some(author) => author,
            None => "Klabnik & Nichols".to_owned(),
        };
        let book = Book {
            id: "book-1".to_owned(),
            title: "The Rust Programming Language".to_owned(),
            author,
            price_cents: 3999,
            tags: query.tag,
        };

        return ListBooksResponse::Ok(vec![book]);
    }

    async fn create_book(&self, headers: CreateBookHeaders, body: NewBook) -> CreateBookResponse {
        if body.title.is_empty() {
            return CreateBookResponse::BadRequest(ErrorResponse {
                code: "empty_title".to_owned(),
                message: "title must not be empty".to_owned(),
            });
        }

        return CreateBookResponse::Created(Book {
            id: headers.idempotency_key,
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

        if id.is_empty() {
            // The `default` response lets the handler pick the status code.
            return GetBookResponse::Default(
                axum::http::StatusCode::BAD_REQUEST,
                ErrorResponse {
                    code: "missing_id".to_owned(),
                    message: "a book id is required".to_owned(),
                },
            );
        }

        return GetBookResponse::NotFound;
    }

    async fn get_health(&self) -> GetHealthResponse {
        return GetHealthResponse::Ok(serde_json::json!({ "status": "ok" }));
    }

    async fn upload_book_cover(&self, id: String, body: UploadBookCoverMultipart) -> UploadBookCoverResponse {
        // The multipart body decodes to a dedicated extractor struct: the binary
        // `image` part is `Vec<u8>`, required text parts are bare, and the
        // optional `caption` is `Option<String>`.
        if id.is_empty() || body.image.is_empty() || body.filename.is_empty() {
            return UploadBookCoverResponse::NotFound;
        }

        let _caption: Option<String> = body.caption;
        return UploadBookCoverResponse::NoContent;
    }

    async fn submit_review(&self, id: String, body: SubmitReviewRequestBody) -> SubmitReviewResponse {
        // The request body decodes from either JSON or a form; both carry the
        // same `NewReview` shape, so collapse them before storing.
        let new_review = match body {
            SubmitReviewRequestBody::Json(review) => review,
            SubmitReviewRequestBody::Form(review) => review,
        };

        if id.is_empty() {
            return SubmitReviewResponse::NotFound;
        }

        let review = Review {
            id: format!("{id}-review-1"),
            rating: new_review.rating,
            comment: new_review.comment,
        };

        // The handler chooses the response representation: terse reviews echo
        // back as plain text, richer ones as structured JSON.
        if review.comment.is_none() {
            return SubmitReviewResponse::Created(SubmitReviewResponseCreatedBody::Text(format!(
                "stored review {} with rating {}",
                review.id, review.rating
            )));
        }

        return SubmitReviewResponse::Created(SubmitReviewResponseCreatedBody::Json(review));
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
