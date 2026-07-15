//! End-to-end integration test: the generated blocking `reqwest` client drives
//! a running instance of the generated axum server over HTTP.
//!
//! This is deliberately excluded from the normal `cargo test` run: it only
//! compiles under the `client` feature, and even then every test is a no-op
//! unless `BOOKSTORE_BASE_URL` points at a running server. The Docker e2e
//! harness (see `crates/oapi-codegen/tests/integration`) sets that variable and
//! runs `cargo test --features client --test e2e`.

#![cfg(feature = "client")]

use std::time::Duration;

use bookstore_example::apimodel::catalog::NewBook;
use bookstore_example::apimodel::catalog::NewReview;
use bookstore_example::restclient::Client;
use bookstore_example::restclient::CreateBookHeaders;
use bookstore_example::restclient::CreateBookResponse;
use bookstore_example::restclient::GetBookResponse;
use bookstore_example::restclient::GetHealthResponse;
use bookstore_example::restclient::ListBooksQuery;
use bookstore_example::restclient::ListBooksResponse;
use bookstore_example::restclient::SubmitReviewRequestBody;
use bookstore_example::restclient::SubmitReviewResponse;
use bookstore_example::restclient::SubmitReviewResponseCreatedBody;
use bookstore_example::restclient::UploadBookCoverMultipart;
use bookstore_example::restclient::UploadBookCoverResponse;

const BASE_URL_ENV: &str = "BOOKSTORE_BASE_URL";
const HEALTH_ATTEMPTS: u32 = 60;
const HEALTH_BACKOFF: Duration = Duration::from_millis(500);

/// Build a client and block until the server answers `/health`, so tests do not
/// race container or server startup.
///
/// Returns `None` when `BOOKSTORE_BASE_URL` is unset, which makes the whole
/// suite a no-op under a plain `cargo test` where no server is running.
fn connected_client() -> Option<Client> {
    let base_url = match std::env::var(BASE_URL_ENV) {
        Ok(base_url) => base_url,
        Err(_) => {
            return None;
        }
    };

    let client = Client::new(base_url).expect("build the blocking reqwest client");
    let mut last_error = String::from("no attempt made");
    for _ in 0..HEALTH_ATTEMPTS {
        match client.get_health() {
            Ok(GetHealthResponse::Ok(_)) => {
                return Some(client);
            }
            Err(error) => {
                last_error = error.to_string();
            }
        }
        std::thread::sleep(HEALTH_BACKOFF);
    }
    panic!("server at {BASE_URL_ENV} never became healthy: {last_error}");
}

/// A per-test unique idempotency key so tests never collide on the shared,
/// stateful server instance.
fn unique_key(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| {
            return elapsed.as_nanos();
        })
        .unwrap_or_default();
    return format!("{prefix}-{nanos}");
}

/// Create a book on the server and return its id.
fn seed_book(client: &Client, prefix: &str) -> String {
    let key = unique_key(prefix);
    let response = client
        .create_book(
            CreateBookHeaders {
                idempotency_key: key.clone(),
            },
            NewBook {
                title: "The Rust Programming Language".to_owned(),
                author: format!("author-{key}"),
                price_cents: 3999,
                tags: Some(vec!["rust".to_owned(), "programming".to_owned()]),
            },
        )
        .expect("create_book request succeeds");

    match response {
        CreateBookResponse::Created(book) => {
            assert_eq!(book.id, key);
            return book.id;
        }
        other => {
            panic!("expected Created, got {other:?}");
        }
    }
}

#[test]
fn health_endpoint_reports_ok() {
    let client = match connected_client() {
        Some(client) => client,
        None => {
            return;
        }
    };

    let response = client.get_health().expect("get_health request succeeds");
    let GetHealthResponse::Ok(document) = response;
    assert_eq!(
        document.get("status").and_then(|value| return value.as_str()),
        Some("ok")
    );
}

#[test]
fn create_then_get_round_trips_a_book() {
    let client = match connected_client() {
        Some(client) => client,
        None => {
            return;
        }
    };

    let id = seed_book(&client, "roundtrip");
    let response = client.get_book(id.clone()).expect("get_book request succeeds");
    match response {
        GetBookResponse::Ok(book) => {
            assert_eq!(book.id, id);
            assert_eq!(book.title, "The Rust Programming Language");
        }
        other => {
            panic!("expected Ok, got {other:?}");
        }
    }
}

#[test]
fn create_rejects_an_empty_title() {
    let client = match connected_client() {
        Some(client) => client,
        None => {
            return;
        }
    };

    let key = unique_key("empty-title");
    let response = client
        .create_book(
            CreateBookHeaders { idempotency_key: key },
            NewBook {
                title: String::new(),
                author: "nobody".to_owned(),
                price_cents: 100,
                tags: None,
            },
        )
        .expect("create_book request succeeds");

    match response {
        CreateBookResponse::BadRequest(error) => {
            assert_eq!(error.code, "empty_title");
        }
        other => {
            panic!("expected BadRequest, got {other:?}");
        }
    }
}

#[test]
fn get_unknown_book_is_not_found() {
    let client = match connected_client() {
        Some(client) => client,
        None => {
            return;
        }
    };

    let response = client
        .get_book(unique_key("missing"))
        .expect("get_book request succeeds");
    match response {
        GetBookResponse::NotFound => {}
        other => {
            panic!("expected NotFound, got {other:?}");
        }
    }
}

#[test]
fn list_filters_by_author() {
    let client = match connected_client() {
        Some(client) => client,
        None => {
            return;
        }
    };

    let id = seed_book(&client, "listfilter");
    let author = format!("author-{id}");
    let response = client
        .list_books(ListBooksQuery {
            author: Some(author.clone()),
            tag: None,
            limit: None,
        })
        .expect("list_books request succeeds");

    let ListBooksResponse::Ok(books) = response;
    assert_eq!(books.len(), 1, "exactly one book carries the unique author");
    assert_eq!(books[0].author, author);
    assert_eq!(books[0].id, id);
}

#[test]
fn upload_cover_for_known_and_unknown_books() {
    let client = match connected_client() {
        Some(client) => client,
        None => {
            return;
        }
    };

    let id = seed_book(&client, "cover");
    let response = client
        .upload_book_cover(
            id,
            UploadBookCoverMultipart {
                image: vec![0x89, 0x50, 0x4E, 0x47],
                filename: "cover.png".to_owned(),
                caption: Some("front cover".to_owned()),
            },
        )
        .expect("upload_book_cover request succeeds");
    match response {
        UploadBookCoverResponse::NoContent => {}
        other => {
            panic!("expected NoContent, got {other:?}");
        }
    }

    let missing = client
        .upload_book_cover(
            unique_key("cover-missing"),
            UploadBookCoverMultipart {
                image: vec![0x00],
                filename: "cover.png".to_owned(),
                caption: None,
            },
        )
        .expect("upload_book_cover request succeeds");
    match missing {
        UploadBookCoverResponse::NotFound => {}
        other => {
            panic!("expected NotFound, got {other:?}");
        }
    }
}

#[test]
fn submit_review_negotiates_json_and_text() {
    let client = match connected_client() {
        Some(client) => client,
        None => {
            return;
        }
    };

    let id = seed_book(&client, "review");

    // A review carrying a comment reads back as structured JSON.
    let json_response = client
        .submit_review(
            id.clone(),
            SubmitReviewRequestBody::Json(NewReview {
                rating: 5,
                comment: Some("superb".to_owned()),
            }),
        )
        .expect("submit_review request succeeds");
    match json_response {
        SubmitReviewResponse::Created(SubmitReviewResponseCreatedBody::Json(review)) => {
            assert_eq!(review.rating, 5);
            assert_eq!(review.comment.as_deref(), Some("superb"));
        }
        other => {
            panic!("expected Created(Json), got {other:?}");
        }
    }

    // A commentless review echoes back as plain text.
    let text_response = client
        .submit_review(
            id,
            SubmitReviewRequestBody::Form(NewReview {
                rating: 3,
                comment: None,
            }),
        )
        .expect("submit_review request succeeds");
    match text_response {
        SubmitReviewResponse::Created(SubmitReviewResponseCreatedBody::Text(text)) => {
            assert!(text.contains("rating 3"), "unexpected text body: {text}");
        }
        other => {
            panic!("expected Created(Text), got {other:?}");
        }
    }
}

#[test]
fn submit_review_for_unknown_book_is_not_found() {
    let client = match connected_client() {
        Some(client) => client,
        None => {
            return;
        }
    };

    let response = client
        .submit_review(
            unique_key("review-missing"),
            SubmitReviewRequestBody::Json(NewReview {
                rating: 1,
                comment: Some("who?".to_owned()),
            }),
        )
        .expect("submit_review request succeeds");
    match response {
        SubmitReviewResponse::NotFound => {}
        other => {
            panic!("expected NotFound, got {other:?}");
        }
    }
}
