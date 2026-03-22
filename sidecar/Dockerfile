# --- Build stage ---
FROM rust:1.87-bookworm AS builder
WORKDIR /src

# Copy workspace manifests first for layer caching
COPY Cargo.toml Cargo.toml
COPY registry/Cargo.toml registry/Cargo.toml
COPY sidecar/Cargo.toml sidecar/Cargo.toml

# Create stub sources so cargo can resolve the workspace
RUN mkdir -p registry/src sidecar/src \
    && echo "fn main() {}" > registry/src/main.rs \
    && echo "fn main() {}" > sidecar/src/main.rs

RUN cargo build --release -p chatixia-sidecar && rm -rf registry/src sidecar/src

# Copy real source and rebuild
COPY registry/src registry/src
COPY sidecar/src sidecar/src
RUN touch sidecar/src/main.rs && cargo build --release -p chatixia-sidecar

# --- Runtime stage ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/chatixia-sidecar /usr/local/bin/chatixia-sidecar

ENV RUST_LOG=info
ENTRYPOINT ["chatixia-sidecar"]
