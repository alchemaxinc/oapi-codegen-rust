# Compile the generated blocking `reqwest` client and its e2e test, then run the
# test against the server named by BOOKSTORE_BASE_URL (set in docker-compose.yml).
# The image keeps the full toolchain because it runs `cargo test` at container
# start; pre-building with `--no-run` means startup goes straight to the tests.
# The build context is the repository root (see docker-compose.yml).

# Rust toolchains can be available before matching Docker image tags.
FROM rust:bookworm AS builder
ARG RUST_VERSION=1.97.1
RUN rustup toolchain install "${RUST_VERSION}" --profile minimal --no-self-update \
    && rustup default "${RUST_VERSION}"
WORKDIR /src
COPY . .
RUN cargo test --no-run --locked -p bookstore-example --features client --test e2e
CMD ["cargo", "test", "--locked", "-p", "bookstore-example", "--features", "client", "--test", "e2e", "--", "--nocapture"]
