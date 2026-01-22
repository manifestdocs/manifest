# Multi-stage Dockerfile for Manifest
# Builds: manifest-docs (MkDocs) → manifest-web (SvelteKit) → manifest-server (Rust)
# The web assets are embedded into the Rust binary via rust-embed

# =============================================================================
# Stage 1: Build documentation with MkDocs
# =============================================================================
FROM python:3.12-slim AS docs-builder

WORKDIR /build

# Install MkDocs and the Material theme
RUN pip install --no-cache-dir mkdocs mkdocs-material

# Copy docs source
COPY manifest-docs ./manifest-docs

# Build documentation
RUN cd manifest-docs && mkdocs build

# =============================================================================
# Stage 2: Build web application with SvelteKit
# =============================================================================
FROM node:22-slim AS web-builder

# Enable corepack for pnpm
RUN corepack enable && corepack prepare pnpm@latest --activate

WORKDIR /build

# Copy package files for dependency caching
COPY manifest-svelte/package.json manifest-svelte/pnpm-lock.yaml ./manifest-svelte/
COPY manifest-web/package.json manifest-web/pnpm-lock.yaml ./manifest-web/

# Install dependencies for both packages
RUN cd manifest-svelte && pnpm install --frozen-lockfile
RUN cd manifest-web && pnpm install --frozen-lockfile

# Copy source files
COPY manifest-svelte ./manifest-svelte
COPY manifest-web ./manifest-web

# Copy docs from previous stage into web static directory
COPY --from=docs-builder /build/manifest-docs/site ./manifest-web/static/docs

# Build shared library first
RUN cd manifest-svelte && pnpm build

# Build web application (output goes to manifest-web/build/)
RUN cd manifest-web && pnpm build

# =============================================================================
# Stage 3: Build Rust application
# =============================================================================
FROM rust:latest AS rust-builder

WORKDIR /app

# Copy Cargo manifests for dependency caching
COPY manifest-server/Cargo.toml manifest-server/Cargo.lock ./
COPY manifest-server/manifest-core/Cargo.toml ./manifest-core/

# Create dummy src to cache dependencies
RUN mkdir -p src manifest-core/src manifest-core/migrations && \
    echo "fn main() {}" > src/main.rs && \
    echo "" > src/lib.rs && \
    echo "" > manifest-core/src/lib.rs

# Build dependencies only (this layer is cached)
RUN cargo build --release && rm -rf src manifest-core/src target/release/deps/manifest*

# Copy actual source
COPY manifest-server/src ./src
COPY manifest-server/manifest-core/src ./manifest-core/src
COPY manifest-server/manifest-core/migrations ./manifest-core/migrations

# Copy built web assets from web-builder stage
# rust-embed expects assets at ../manifest-web/build relative to manifest-server
COPY --from=web-builder /build/manifest-web/build ../manifest-web/build

# Build the release binary
RUN cargo build --release

# =============================================================================
# Stage 4: Runtime image
# =============================================================================
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libsqlite3-0 \
    libpq5 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -s /bin/bash mfst

# Create data directory
RUN mkdir -p /data && chown mfst:mfst /data

# Copy binary from builder
COPY --from=rust-builder /app/target/release/manifest /usr/local/bin/manifest

# Switch to non-root user
USER mfst

# Set data directory and bind to all interfaces for container networking
ENV MANIFEST_DATA_DIR=/data
ENV MANIFEST_BIND_ADDR=0.0.0.0

# Expose port
EXPOSE 17010

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:17010/api/v1/health || exit 1

# Run the server
ENTRYPOINT ["manifest"]
CMD ["serve", "--port", "17010"]
