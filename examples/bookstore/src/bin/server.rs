//! A runnable bookstore server used by the Docker e2e integration test.
//!
//! It wires the shared in-memory [`bookstore_example::Service`] into the
//! generated axum router and serves it. The bind address comes from
//! `BOOKSTORE_ADDR` (default `0.0.0.0:8080`) so the container can override it.

use std::net::SocketAddr;

use bookstore_example::Service;
use bookstore_example::restapi;

const DEFAULT_ADDR: &str = "0.0.0.0:8080";

#[tokio::main]
async fn main() {
    let addr = bind_address();
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind the bookstore server listener");

    let local = listener.local_addr().expect("resolve the bound listener address");
    #[allow(
        clippy::print_stdout,
        reason = "intentional startup banner for a runnable server binary, not a debug leftover"
    )]
    {
        println!("bookstore server listening on http://{local}");
    }

    axum::serve(listener, restapi::router(Service::new()))
        .await
        .expect("serve the bookstore router");
}

/// Resolve the socket address to bind, honouring `BOOKSTORE_ADDR`.
fn bind_address() -> SocketAddr {
    let raw = std::env::var("BOOKSTORE_ADDR").unwrap_or_else(|_| {
        return DEFAULT_ADDR.to_owned();
    });
    return raw
        .parse()
        .unwrap_or_else(|error| panic!("BOOKSTORE_ADDR ({raw}) is not a valid socket address: {error}"));
}
