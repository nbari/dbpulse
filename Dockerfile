# Multi-stage Dockerfile for production dbpulse container
# This creates a minimal container with just the binary

# Stage 1: Build
FROM rust:1-alpine AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev pkgconfig openssl-dev openssl-libs-static

WORKDIR /build

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build for the native architecture (Docker buildx handles platform selection)
# The rust image will be the correct architecture automatically
RUN ARCH=$(uname -m) && \
    if [ "$ARCH" = "x86_64" ]; then \
        RUST_TARGET="x86_64-unknown-linux-musl"; \
    elif [ "$ARCH" = "aarch64" ]; then \
        RUST_TARGET="aarch64-unknown-linux-musl"; \
    else \
        echo "Unsupported architecture: $ARCH" && exit 1; \
    fi && \
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
