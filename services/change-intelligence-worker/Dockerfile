# Build stage
FROM rust:1.86-bookworm AS builder

WORKDIR /app

# Copy the entire workspace since gix dependencies need it
COPY . .

# Build the change-intelligence-worker binary
RUN cargo build \
    --release \
    --package change-intelligence-worker \
    --bin change-intelligence-worker

# Runtime stage
FROM debian:bookworm-slim

# Install git and CA certificates for HTTPS
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        git \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the built binary
COPY --from=builder \
    /app/target/release/change-intelligence-worker \
    /usr/local/bin/worker

# Create repository cache directory
RUN mkdir -p /data/repos

# Set default environment variables
ENV REPOSITORY_CACHE=/data/repos
ENV HEALTH_PORT=8080
ENV RUST_LOG=info

# Expose health check port
EXPOSE 8080

# Run the worker
CMD ["/usr/local/bin/worker"]
