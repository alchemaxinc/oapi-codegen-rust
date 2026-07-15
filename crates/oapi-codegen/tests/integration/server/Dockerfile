# Build the bookstore server binary from the workspace, then run it from a slim
# runtime image. The build context is the repository root (see docker-compose.yml).

FROM rust:1.96-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p bookstore-example --bin bookstore-server

FROM debian:bookworm-slim AS runtime
WORKDIR /app
COPY --from=builder /src/target/release/bookstore-server /usr/local/bin/bookstore-server
ENV BOOKSTORE_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["bookstore-server"]
