# Multi-stage Dockerfile for production dbpulse container
# This creates a minimal container with just the binary

# Stage 1: Build
FROM --platform=$BUILDPLATFORM rust:1-slim AS builder

ARG TARGETPLATFORM
ARG BUILDPLATFORM

# Install build dependencies
RUN apt-get update && apt-get install -y \
    musl-tools \
    musl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Determine target architecture and build
RUN case "$TARGETPLATFORM" in \
      "linux/amd64") \
        RUST_TARGET=x86_64-unknown-linux-musl ;; \
      "linux/arm64") \
        RUST_TARGET=aarch64-unknown-linux-musl ;; \
      *) \
        echo "Unsupported platform: $TARGETPLATFORM" && exit 1 ;; \
    esac && \
    rustup target add ${RUST_TARGET} && \
    cargo build --release --target ${RUST_TARGET} --locked && \
    strip /build/target/${RUST_TARGET}/release/dbpulse && \
    cp /build/target/${RUST_TARGET}/release/dbpulse /build/dbpulse

# Stage 2: Runtime
FROM scratch

# Copy CA certificates for TLS connections
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy the binary
COPY --from=builder /build/dbpulse /usr/local/bin/dbpulse

# Expose default metrics port
EXPOSE 9300

# Set user (non-root)
USER 65534:65534

# Run dbpulse
ENTRYPOINT ["/usr/local/bin/dbpulse"]
CMD ["--help"]
