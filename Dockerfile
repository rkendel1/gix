FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --package change-intelligence-worker --bin change-intelligence-worker

# We do not need the Rust toolchain to run the binary!
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# Install git and CA certificates for HTTPS
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        git \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create repository cache directory
RUN mkdir -p /data/repos

# Expose health check port
EXPOSE 8080

COPY --from=builder /app/target/release/change-intelligence-worker /usr/local/bin/worker
CMD ["/usr/local/bin/worker"]
