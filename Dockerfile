# syntax=docker/dockerfile:1
# Stage 1: Build the Rust application
FROM rust:latest AS builder
WORKDIR /app

# Pre-build the library dependencies
COPY Cargo.toml Cargo.lock .
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --bin bankie

# Copy everything from the current directory to the PWD (Present Working Directory) inside the container
COPY src src
COPY config.*.yaml .
COPY .sqlx .sqlx

ENV SQLX_OFFLINE=true

RUN touch src/main.rs
RUN cargo build --release --bin bankie

RUN strip target/release/bankie

# Stage 2: Create a smaller image with the built binary
FROM gcr.io/distroless/cc-debian12 AS release

# Install necessary runtime dependencies
# RUN apt-get update && apt-get install -y ca-certificates && apt clean && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/bankie /app/bankie
COPY --from=builder /app/config.*.yaml /app/

# This container exposes ports to the outside world
EXPOSE 80 443 3030

CMD ["/app/bankie", "--mode", "server"]
