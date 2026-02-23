# syntax=docker/dockerfile:1
# Stage 1: Build the Rust application
FROM rust:latest AS builder
WORKDIR /app

# Pre-build the library dependencies
COPY Cargo.toml Cargo.lock .
RUN mkdir -p src/repository && \
    echo "fn main() {}" > src/main.rs && \
    echo "fn main() {}" > src/repository/migrate.rs
RUN cargo build --release --bin bankie --bin migrations

# Copy everything from the current directory to the PWD (Present Working Directory) inside the container
COPY src src
COPY config.*.yaml .
COPY .sqlx .sqlx

ENV SQLX_OFFLINE=true

RUN touch src/main.rs src/repository/migrate.rs
RUN cargo build --release --bin bankie --bin migrations

RUN strip target/release/bankie target/release/migrations

# Stage 2: Migrations runner (needs full OS for DB tools)
FROM debian:bookworm-slim AS migrations

RUN apt-get update && apt-get install -y ca-certificates postgresql-client && \
    apt-get clean && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/migrations /app/migrations
COPY db/init.sql /app/db/init.sql
COPY db/migrations /app/db/migrations

# Stage 3: Create a smaller image with the built binary
FROM gcr.io/distroless/cc-debian12 AS release

WORKDIR /app

COPY --from=builder /app/target/release/bankie /app/bankie
COPY --from=builder /app/config.*.yaml /app/

# This container exposes ports to the outside world
EXPOSE 80 443 3030

CMD ["/app/bankie", "--mode", "server"]
