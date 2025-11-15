# Production Dockerfile for dbpulse
# Uses pre-built binaries from GitHub Actions workflow for fast multi-arch builds
# For local development, use: cargo build --release

FROM scratch
ARG TARGETARCH

# Copy CA certificates for HTTPS/TLS connections
COPY --from=alpine:latest /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy pre-built binary for the target architecture
# Binaries are provided by the GitHub Actions build workflow
COPY bin/${TARGETARCH}/dbpulse /usr/local/bin/dbpulse

# Expose default metrics port
EXPOSE 9300

# Run as non-root user (nobody:nogroup)
USER 65534:65534

ENTRYPOINT ["/usr/local/bin/dbpulse"]
CMD ["--help"]
