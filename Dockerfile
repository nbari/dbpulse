# Multi-stage Dockerfile for production dbpulse container
# This creates a minimal container with just the binary

# Stage 1: Planner - analyze dependencies
FROM rust:1-alpine AS planner
RUN apk add --no-cache musl-dev
RUN cargo install cargo-chef
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Builder - build dependencies (cached layer)
FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev pkgconfig openssl-dev openssl-libs-static
RUN cargo install cargo-chef
WORKDIR /build

# Determine target architecture
RUN ARCH=$(uname -m) && \
    if [ "$ARCH" = "x86_64" ]; then \
        echo "x86_64-unknown-linux-musl" > /tmp/rust_target; \
    elif [ "$ARCH" = "aarch64" ]; then \
        echo "aarch64-unknown-linux-musl" > /tmp/rust_target; \
    else \
        echo "Unsupported architecture: $ARCH" && exit 1; \
    fi

# Build dependencies - this is the cacheable layer
COPY --from=planner /build/recipe.json recipe.json
RUN RUST_TARGET=$(cat /tmp/rust_target) && \
    rustup target add ${RUST_TARGET} && \
    cargo chef cook --release --target ${RUST_TARGET} --recipe-path recipe.json

# Build application
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN RUST_TARGET=$(cat /tmp/rust_target) && \
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
