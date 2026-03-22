# --- Hub build stage ---
FROM node:22-slim AS hub-builder
RUN corepack enable
WORKDIR /app
COPY hub/package.json hub/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY hub/ .
RUN pnpm build

# --- Rust build stage ---
FROM rust:1.87-bookworm AS rust-builder
WORKDIR /src

# Copy workspace manifests first for layer caching
COPY Cargo.toml Cargo.toml
COPY registry/Cargo.toml registry/Cargo.toml
COPY sidecar/Cargo.toml sidecar/Cargo.toml

# Create stub sources so cargo can resolve the workspace
RUN mkdir -p registry/src sidecar/src \
    && echo "fn main() {}" > registry/src/main.rs \
    && echo "fn main() {}" > sidecar/src/main.rs

RUN cargo build --release -p chatixia-registry && rm -rf registry/src sidecar/src

# Copy real source and rebuild
COPY registry/src registry/src
COPY sidecar/src sidecar/src
RUN touch registry/src/main.rs && cargo build --release -p chatixia-registry

# --- Runtime stage ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=rust-builder /src/target/release/chatixia-registry /usr/local/bin/chatixia-registry
COPY --from=hub-builder /app/dist /srv/hub

ENV RUST_LOG=info
ENV HUB_DIST_DIR=/srv/hub
EXPOSE 8080
ENTRYPOINT ["chatixia-registry"]
