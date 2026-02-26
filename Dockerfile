# syntax=docker/dockerfile:1
# Stage 1: Build the Rust application
FROM rust:latest AS builder
WORKDIR /app

# Copy workspace manifest and crate manifests for dependency caching
COPY Cargo.toml Cargo.lock .
COPY crates/bankie-common/Cargo.toml crates/bankie-common/Cargo.toml
COPY crates/bankie-core/Cargo.toml crates/bankie-core/Cargo.toml
COPY crates/bankie-gateway/Cargo.toml crates/bankie-gateway/Cargo.toml

# Create dummy source files for dependency pre-build
RUN mkdir -p crates/bankie-common/src && \
    echo "pub mod error;" > crates/bankie-common/src/lib.rs && \
    echo "pub enum AppError {}" > crates/bankie-common/src/error.rs && \
    mkdir -p crates/bankie-core/src/repository && \
    echo "fn main() {}" > crates/bankie-core/src/main.rs && \
    echo "fn main() {}" > crates/bankie-core/src/repository/migrate.rs && \
    mkdir -p crates/bankie-gateway/src && \
    echo "fn main() {}" > crates/bankie-gateway/src/main.rs && \
    echo "" > crates/bankie-gateway/src/lib.rs
RUN cargo build --release --bin bankie --bin migrations --bin bankie-gateway || true

# Copy real source code
COPY crates crates
COPY config.*.yaml .
COPY crates/bankie-core/.sqlx crates/bankie-core/.sqlx

ENV SQLX_OFFLINE=true

RUN touch crates/bankie-common/src/lib.rs crates/bankie-core/src/main.rs crates/bankie-core/src/repository/migrate.rs crates/bankie-gateway/src/main.rs crates/bankie-gateway/src/lib.rs
RUN cargo build --release --bin bankie --bin migrations --bin bankie-gateway

RUN strip target/release/bankie target/release/migrations target/release/bankie-gateway

# Stage 2: Migrations runner (needs full OS for DB tools)
FROM debian:bookworm-slim AS migrations

RUN apt-get update && apt-get install -y ca-certificates postgresql-client && \
    apt-get clean && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/migrations /app/migrations
COPY db/init.sql /app/db/init.sql
COPY db/migrations /app/db/migrations

# Stage 3: Bankie Core
FROM gcr.io/distroless/cc-debian12 AS release

WORKDIR /app

COPY --from=builder /app/target/release/bankie /app/bankie
COPY --from=builder /app/config.*.yaml /app/

# This container exposes ports to the outside world
EXPOSE 80 443 3030

CMD ["/app/bankie", "--mode", "server"]

# Stage 4: Gateway
FROM gcr.io/distroless/cc-debian12 AS gateway

WORKDIR /app

COPY --from=builder /app/target/release/bankie-gateway /app/bankie-gateway
COPY --from=builder /app/config.*.yaml /app/

EXPOSE 4040

CMD ["/app/bankie-gateway"]
