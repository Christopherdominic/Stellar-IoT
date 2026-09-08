# ---- Build stage ----------------------------------------------------------
# Track the latest stable 1.x toolchain: a pinned older tag can fall behind
# transitive dependencies that require newer edition/MSRV support.
FROM rust:1-bookworm AS builder
WORKDIR /app

# Cache dependency compilation separately from source changes: build a dummy
# binary against just the manifest first, then overwrite src/ and rebuild.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
# Touch main.rs so cargo doesn't reuse the dummy build's stale timestamp.
RUN touch src/main.rs && cargo build --release

# ---- Runtime stage ----------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/stellar-iot-api ./stellar-iot-api

ENV RUST_LOG=info
EXPOSE 8000
CMD ["./stellar-iot-api"]
