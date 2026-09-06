# Build the bookstore server binary from the workspace, then run it from a slim
# runtime image. The build context is the repository root (see docker-compose.yml).

# Rust toolchains can be available before matching Docker image tags.
FROM rust:bookworm AS builder
ARG RUST_VERSION=1.97.1
RUN rustup toolchain install "${RUST_VERSION}" --profile minimal --no-self-update \
    && rustup default "${RUST_VERSION}"
WORKDIR /src
COPY . .
RUN cargo build --locked -p bookstore-example --bin bookstore-server

FROM debian:bookworm-slim AS runtime
WORKDIR /app
COPY --from=builder /src/target/debug/bookstore-server /usr/local/bin/bookstore-server
ENV BOOKSTORE_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["bookstore-server"]
