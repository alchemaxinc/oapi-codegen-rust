//! An in-memory implementation of the generated [`crate::restapi::Api`] trait.
//!
//! This backs both the smoke test (which proves the trait is implementable and
//! the router builds) and the `bookstore-server` binary used by the Docker e2e
//! integration test. State is a `Mutex`-guarded map so the type stays `Clone +
//! Send + Sync + 'static` as the trait requires; the lock is never held across
//! an `.await`, so each operation's future remains `Send`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use crate::apimodel::catalog::Book;
use crate::apimodel::catalog::NewBook;
use crate::apimodel::catalog::Review;
use crate::apimodel::common::ErrorResponse;
use crate::restapi::Api;
use crate::restapi::CreateBookHeaders;
use crate::restapi::CreateBookResponse;
use crate::restapi::GetBookResponse;
use crate::restapi::GetHealthResponse;
use crate::restapi::ListBooksQuery;
use crate::restapi::ListBooksResponse;
use crate::restapi::SubmitReviewRequestBody;
use crate::restapi::SubmitReviewResponse;
use crate::restapi::SubmitReviewResponseCreatedBody;
use crate::restapi::UploadBookCoverMultipart;
use crate::restapi::UploadBookCoverResponse;

/// A negative `limit` query parameter is clamped to this floor before being
/// used as a `Vec::truncate` bound.
const MIN_LIMIT: i32 = 0;

/// An in-memory bookstore backing the generated [`Api`] trait.
#[derive(Clone, Default, Debug)]
pub struct Service {
    books: Arc<Mutex<HashMap<String, Book>>>,
}

impl Service {
    /// Create an empty bookstore.
    pub fn new() -> Self {
        return Self::default();
    }

    /// Lock the book map, recovering the inner value if a previous holder
    /// panicked so a poisoned lock cannot take the whole server down.
    fn books(&self) -> std::sync::MutexGuard<'_, HashMap<String, Book>> {
        match self.books.lock() {
            Ok(guard) => {
                return guard;
            }
            Err(poisoned) => {
                return poisoned.into_inner();
            }
        }
    }
}

impl Api for Service {
    async fn list_books(&self, query: ListBooksQuery) -> ListBooksResponse {
        let mut result: Vec<Book> = {
            let books = self.books();
            let mut matched: Vec<Book> = Vec::new();
            for book in books.values() {
                if let Some(author) = &query.author
                    && &book.author != author
                {
                    continue;
                }
                if let Some(required) = &query.tag
                    && !has_all_tags(book, required)
                {
                    continue;
                }
                matched.push(book.clone());
            }
            matched
        };

        result.sort_by(|left, right| {
            return left.id.cmp(&right.id);
        });
        if let Some(limit) = query.limit {
            result.truncate(limit.max(MIN_LIMIT) as usize);
        }
        return ListBooksResponse::Ok(result);
    }

    async fn create_book(&self, headers: CreateBookHeaders, body: NewBook) -> CreateBookResponse {
        if body.title.trim().is_empty() {
            return CreateBookResponse::BadRequest(ErrorResponse {
                code: "empty_title".to_owned(),
                message: "title must not be empty".to_owned(),
            });
        }

        let book = Book {
            id: headers.idempotency_key,
            title: body.title,
            author: body.author,
            price_cents: body.price_cents,
            tags: body.tags,
        };
        {
            let mut books = self.books();
            books.insert(book.id.clone(), book.clone());
        }
        return CreateBookResponse::Created(book);
    }

    async fn get_book(&self, id: String) -> GetBookResponse {
        let found = {
            let books = self.books();
            books.get(&id).cloned()
        };
        match found {
            Some(book) => {
                return GetBookResponse::Ok(book);
            }
            None => {
                return GetBookResponse::NotFound;
            }
        }
    }

    async fn upload_book_cover(&self, id: String, body: UploadBookCoverMultipart) -> UploadBookCoverResponse {
        let exists = {
            let books = self.books();
            books.contains_key(&id)
        };
        if !exists || body.image.is_empty() || body.filename.is_empty() {
            return UploadBookCoverResponse::NotFound;
        }
        return UploadBookCoverResponse::NoContent;
    }

    async fn submit_review(&self, id: String, body: SubmitReviewRequestBody) -> SubmitReviewResponse {
        let exists = {
            let books = self.books();
            books.contains_key(&id)
        };
        if !exists {
            return SubmitReviewResponse::NotFound;
        }

        let new_review = match body {
            SubmitReviewRequestBody::Json(review) => review,
            SubmitReviewRequestBody::Form(review) => review,
        };
        let review = Review {
            id: format!("{id}-review-1"),
            rating: new_review.rating,
            comment: new_review.comment,
        };

        if review.comment.is_none() {
            return SubmitReviewResponse::Created(SubmitReviewResponseCreatedBody::Text(format!(
                "stored review {} with rating {}",
                review.id, review.rating
            )));
        }
        return SubmitReviewResponse::Created(SubmitReviewResponseCreatedBody::Json(review));
    }

    async fn get_health(&self) -> GetHealthResponse {
        return GetHealthResponse::Ok(serde_json::json!({ "status": "ok" }));
    }
}

/// Whether `book` carries every tag in `required`.
fn has_all_tags(book: &Book, required: &[String]) -> bool {
    let book_tags = match &book.tags {
        Some(book_tags) => book_tags,
        None => {
            return required.is_empty();
        }
    };
    for tag in required {
        if !book_tags.contains(tag) {
            return false;
        }
    }
    return true;
}
