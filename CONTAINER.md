# Container Images for dbpulse

This document describes how to build, publish, and use dbpulse container images.

## Available Images

### Production Image (Minimal - ~5MB)
- **File**: `Dockerfile.production`
- **Base**: `FROM scratch`
- **Size**: ~5-8 MB
- **Use Case**: Production deployments, minimal attack surface
- **Features**:
  - Static binary
  - No shell
  - Runs as non-root user (65534)
  - Only contains the binary and CA certificates

### Alpine Image (~15MB)
- **File**: `Dockerfile.alpine`
- **Base**: `alpine:latest`
- **Size**: ~15-20 MB
- **Use Case**: Development, debugging, when you need shell access
- **Features**:
  - Includes shell and basic tools
  - Health check built-in
  - Runs as non-root user `dbpulse` (UID 1000)
  - Easy to extend

## Building Locally

### Using Just

```bash
# Build production image (minimal)
just build-container-production

# Build alpine image
just build-container-alpine

# Build both
just build-containers

# Test the images
just test-container-production
just test-container-alpine
```

### Using Podman/Docker Directly

```bash
# Production image
podman build -f Dockerfile.production -t dbpulse:latest .

# Alpine image
podman build -f Dockerfile.alpine -t dbpulse:alpine .
```

## Running Containers

### Basic Usage

```bash
# Show help
podman run --rm dbpulse:latest --help

# Check version
podman run --rm dbpulse:latest --version
```

### With PostgreSQL

```bash
podman run --rm \
  --network=host \
  dbpulse:latest \
  --dsn "postgres://postgres:secret@tcp(localhost:5432)/testdb" \
  --interval 5 \
  --range 100
```

### With MariaDB/MySQL

```bash
podman run --rm \
  --network=host \
  dbpulse:latest \
  --dsn "mysql://user:pass@tcp(localhost:3306)/dbname" \
  --interval 10
```

### With Custom Port for Metrics

```bash
podman run --rm \
  -p 8080:9300 \
  --network=host \
  dbpulse:latest \
  --dsn "postgres://postgres:secret@tcp(localhost:5432)/testdb" \
  --port 9300
```

### As a Daemon (Detached)

```bash
podman run -d \
  --name dbpulse \
  --network=host \
  --restart=unless-stopped \
  dbpulse:latest \
  --dsn "postgres://postgres:secret@tcp(localhost:5432)/testdb" \
  --interval 30
```

## Publishing to Registries

### GitHub Container Registry (GHCR)

Automated via GitHub Actions on push to main or tags:

```bash
# Triggered automatically on:
# - Push to main branch
# - Push of version tags (v*)
# - Manual workflow_dispatch

# Images are published to:
# ghcr.io/OWNER/dbpulse:latest
# ghcr.io/OWNER/dbpulse:alpine
# ghcr.io/OWNER/dbpulse:v1.2.3
# ghcr.io/OWNER/dbpulse:v1.2.3-alpine
```

### Manual Push to GHCR

```bash
# Login
echo $GITHUB_TOKEN | podman login ghcr.io -u USERNAME --password-stdin

# Tag
podman tag dbpulse:latest ghcr.io/USERNAME/dbpulse:latest

# Push
podman push ghcr.io/USERNAME/dbpulse:latest
```

### Docker Hub

```bash
# Login
podman login docker.io

# Tag
podman tag dbpulse:latest docker.io/USERNAME/dbpulse:latest

# Push
podman push docker.io/USERNAME/dbpulse:latest
```

## Pulling and Using Published Images

### From GitHub Container Registry

```bash
# Pull latest
podman pull ghcr.io/nbari/dbpulse:latest

# Pull specific version
podman pull ghcr.io/nbari/dbpulse:v1.0.0

# Pull alpine variant
podman pull ghcr.io/nbari/dbpulse:alpine

# Run
podman run --rm ghcr.io/nbari/dbpulse:latest --help
```

### From Docker Hub

```bash
# Pull
podman pull docker.io/nbari/dbpulse:latest

# Run
podman run --rm docker.io/nbari/dbpulse:latest --help
```

## Docker Compose / Podman Compose

Example `docker-compose.yml`:

```yaml
version: '3.8'

services:
  dbpulse:
    image: ghcr.io/nbari/dbpulse:latest
    container_name: dbpulse
    restart: unless-stopped
    network_mode: host
    command:
      - --dsn
      - postgres://postgres:secret@tcp(localhost:5432)/mydb
      - --interval
      - "30"
      - --range
      - "100"
    ports:
      - "9300:9300"
```

## Kubernetes Deployment

Example deployment:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: dbpulse
spec:
  replicas: 1
  selector:
    matchLabels:
      app: dbpulse
  template:
    metadata:
      labels:
        app: dbpulse
    spec:
      containers:
      - name: dbpulse
        image: ghcr.io/nbari/dbpulse:latest
        args:
          - --dsn
          - postgres://postgres:secret@tcp(postgres-service:5432)/mydb
          - --interval
          - "30"
        ports:
        - containerPort: 9300
          name: metrics
        resources:
          limits:
            memory: "64Mi"
            cpu: "100m"
          requests:
            memory: "32Mi"
            cpu: "50m"
        securityContext:
          runAsNonRoot: true
          runAsUser: 65534
          readOnlyRootFilesystem: true
---
apiVersion: v1
kind: Service
metadata:
  name: dbpulse-metrics
spec:
  selector:
    app: dbpulse
  ports:
  - port: 9300
    targetPort: 9300
    name: metrics
```

## Multi-Architecture Support

The GitHub Actions workflow builds for multiple architectures:

**Production image:**
- linux/amd64 (x86_64)
- linux/arm64 (ARM64/aarch64)

**Alpine image:**
- linux/amd64 (x86_64)
- linux/arm64 (ARM64/aarch64)
- linux/arm/v7 (ARM 32-bit)

Pull the appropriate image for your platform:

```bash
# Automatically pulls the right architecture
podman pull ghcr.io/nbari/dbpulse:latest

# Verify architecture
podman inspect ghcr.io/nbari/dbpulse:latest | grep Architecture
```

## Security Considerations

1. **Non-root User**: All images run as non-root users
2. **Minimal Base**: Production image uses `FROM scratch`
3. **Static Binary**: No dynamic dependencies
4. **CA Certificates**: Included for TLS connections
5. **No Shell**: Production image has no shell (prevents shell exploits)
6. **Read-only Root**: Can be run with read-only root filesystem

## Image Size Comparison

| Image | Uncompressed | Compressed |
|-------|-------------|------------|
| Production (scratch) | ~5-8 MB | ~2-3 MB |
| Alpine | ~15-20 MB | ~5-8 MB |
| Full (with build tools) | ~1.5 GB | ~500 MB |

## Troubleshooting

### Container won't start

```bash
# Check logs
podman logs dbpulse

# Run with shell (alpine only)
podman run --rm -it --entrypoint /bin/sh dbpulse:alpine

# Check if port is available
netstat -tulpn | grep 9300
```

### Can't connect to database

```bash
# Test database connectivity from host
psql -h localhost -U postgres -d testdb

# Use host network mode
podman run --network=host ...

# Check DSN format
--dsn "postgres://user:pass@tcp(host:5432)/db"
```

### Permission denied

```bash
# Production image runs as user 65534
# Make sure no privileged operations are needed

# Alpine runs as user 1000
# Adjust volumes/mounts accordingly
```

## Environment Variables

Containers support environment variables:

```bash
podman run --rm \
  -e DBPULSE_DSN="postgres://..." \
  -e DBPULSE_INTERVAL=30 \
  -e DBPULSE_PORT=9300 \
  dbpulse:latest
```

## Best Practices

1. **Use specific tags** - Don't use `:latest` in production
2. **Pin versions** - Use semantic version tags (`:v1.2.3`)
3. **Health checks** - Alpine image includes health checks
4. **Resource limits** - Set memory/CPU limits in Kubernetes
5. **Secrets** - Use Kubernetes secrets or env vars, not DSN in args
6. **Monitoring** - Scrape metrics endpoint at `/metrics`
7. **Updates** - Use dependabot or renovate to track updates

## Additional Resources

- [GitHub Container Registry Docs](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry)
- [Podman Documentation](https://docs.podman.io/)
- [Docker Documentation](https://docs.docker.com/)
- [Kubernetes Documentation](https://kubernetes.io/docs/)
