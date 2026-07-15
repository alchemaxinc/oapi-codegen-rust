# Compile the generated blocking `reqwest` client and its e2e test, then run the
# test against the server named by BOOKSTORE_BASE_URL (set in docker-compose.yml).
# The image keeps the full toolchain because it runs `cargo test` at container
# start; pre-building with `--no-run` means startup goes straight to the tests.
# The build context is the repository root (see docker-compose.yml).

FROM rust:1.96-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo test --no-run --locked -p bookstore-example --features client --test e2e
CMD ["cargo", "test", "--locked", "-p", "bookstore-example", "--features", "client", "--test", "e2e", "--", "--nocapture"]
