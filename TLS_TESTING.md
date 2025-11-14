# TLS Integration Testing Guide

This guide explains how to run TLS integration tests.

## Quick Start

**Test everything (recommended):**
```bash
just test
```

**Test only TLS:**
```bash
just test-tls
```

## What Gets Tested

**PostgreSQL TLS Tests (8 tests):**
- TLS modes: disable, require, verify-ca, verify-full
- Multiple connections, wrong CA validation
- TLS metadata extraction (version, cipher)

**MariaDB TLS Tests (9 tests):**
- TLS modes: disable, required, verify-ca, verify-identity
- Multiple connections, wrong CA validation
- Cipher suite verification

## How It Works

`just test` or `just test-tls` automatically:
1. Generates self-signed TLS certificates
2. Builds PostgreSQL container with proper key ownership
3. Starts PostgreSQL and MariaDB with TLS enabled
4. Runs all tests
5. Cleans up everything

No manual setup required!

## Troubleshooting

```bash
# Check certificates
openssl verify -CAfile .certs/postgres/ca.crt .certs/postgres/server.crt

# View container logs
podman logs dbpulse-postgres-tls
podman logs dbpulse-mariadb-tls

# Clean up manually
podman rm -f dbpulse-postgres-tls dbpulse-mariadb-tls
rm -rf .certs Dockerfile.postgres-tls
```

## Advanced: Manual Container Setup

If you need to start the containers manually:

```bash
# Generate certificates
./scripts/gen-certs.sh
chmod 644 .certs/mariadb/server.key

# Build PostgreSQL with proper permissions
cat > Dockerfile.postgres-tls <<'EOF'
FROM postgres:17-alpine
COPY .certs/postgres/server.crt /var/lib/postgresql/server.crt
COPY .certs/postgres/server.key /var/lib/postgresql/server.key
COPY .certs/postgres/ca.crt /var/lib/postgresql/ca.crt
RUN chown postgres:postgres /var/lib/postgresql/server.* /var/lib/postgresql/ca.crt && \
    chmod 600 /var/lib/postgresql/server.key && \
    chmod 644 /var/lib/postgresql/server.crt /var/lib/postgresql/ca.crt
EOF
podman build -t postgres-tls:test -f Dockerfile.postgres-tls .

# Start PostgreSQL
podman run -d --name dbpulse-postgres-tls \
  -e POSTGRES_USER=postgres -e POSTGRES_PASSWORD=secret -e POSTGRES_DB=testdb \
  -p 5432:5432 postgres-tls:test \
  -c ssl=on -c ssl_cert_file=/var/lib/postgresql/server.crt \
  -c ssl_key_file=/var/lib/postgresql/server.key \
  -c ssl_ca_file=/var/lib/postgresql/ca.crt

# Start MariaDB
podman run -d --name dbpulse-mariadb-tls \
  -e MARIADB_USER=dbpulse -e MARIADB_PASSWORD=secret \
  -e MARIADB_ROOT_PASSWORD=secret -e MARIADB_DATABASE=testdb \
  -p 3306:3306 \
  -v $(pwd)/.certs/mariadb:/etc/mysql/ssl:ro \
  mariadb:11 --ssl-cert=/etc/mysql/ssl/server.crt \
  --ssl-key=/etc/mysql/ssl/server.key --ssl-ca=/etc/mysql/ssl/ca.crt

# Cleanup
podman rm -f dbpulse-postgres-tls dbpulse-mariadb-tls
```

For detailed script documentation, see [scripts/README.md](scripts/README.md).
