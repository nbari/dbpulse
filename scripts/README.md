# Integration Testing Scripts

This directory contains the certificate generation script for TLS integration testing.

## Overview

TLS testing is fully integrated into the justfile. No manual script execution needed!

## Quick Start

```bash
# Run all tests (unit, integration, TLS)
just test

# Run only TLS tests
just test-tls

# Run only integration tests (non-TLS)
just test-integration
```

The justfile automatically:
- ✅ Generates TLS certificates
- ✅ Starts containers with proper configuration
- ✅ Waits for databases to be ready
- ✅ Runs tests
- ✅ Cleans up everything

## Scripts

### `gen-certs.sh`

Generates self-signed TLS certificates for both PostgreSQL and MariaDB.

**Output structure:**
```
.certs/
├── postgres/
│   ├── ca.crt          # Certificate Authority
│   ├── ca.key          # CA private key
│   ├── server.crt      # Server certificate
│   ├── server.key      # Server private key
│   ├── client.crt      # Client certificate (optional)
│   └── client.key      # Client private key (optional)
└── mariadb/
    ├── ca.crt
    ├── ca.key
    ├── server.crt
    ├── server.key
    ├── client.crt
    └── client.key
```

**Features:**
- 10-year validity period
- Subject Alternative Names (SAN) for localhost, DNS names, and IP
- Proper permissions for database servers
- OpenSSL verification

## TLS Container Configuration

TLS-enabled databases are managed directly by the justfile using Podman.

**What gets created:**

#### PostgreSQL
- Image: Custom-built `postgres-tls:test` (proper key ownership)
- Port: `5432`
- TLS: Enabled (TLSv1.2, TLSv1.3)
- Container name: `dbpulse-postgres-tls`

#### MariaDB
- Image: `mariadb:11`
- Port: `3306`
- TLS: Enabled (TLSv1.2, TLSv1.3)
- Configuration:
  - SSL available but not required
  - CA certificate validation
  - Self-signed server certificate

#### MariaDB
- Image: `mariadb:11`
- Port: `3306`
- TLS: Enabled (TLSv1.2, TLSv1.3)
- Container name: `dbpulse-mariadb-tls`

**Manual access:**
```bash
# View PostgreSQL SSL status
podman exec dbpulse-postgres-tls psql -U postgres -d testdb -c "SHOW ssl;"

# View MariaDB SSL status
podman exec dbpulse-mariadb-tls mariadb -u root -psecret -e "SHOW VARIABLES LIKE 'have_ssl';"
```

## Connection Strings

### PostgreSQL

```bash
# TLS required
postgresql://postgres:secret@tcp(localhost:5432)/testdb?sslmode=require

# TLS with CA verification
postgresql://postgres:secret@tcp(localhost:5432)/testdb?sslmode=verify-ca&sslrootcert=.certs/postgres/ca.crt

# TLS with full verification
postgresql://postgres:secret@tcp(localhost:5432)/testdb?sslmode=verify-full&sslrootcert=.certs/postgres/ca.crt
```

### MariaDB/MySQL

```bash
# TLS required
mysql://dbpulse:secret@tcp(localhost:3306)/testdb?ssl-mode=REQUIRED

# TLS with CA verification
mysql://dbpulse:secret@tcp(localhost:3306)/testdb?ssl-mode=VERIFY_CA&ssl-ca=.certs/mariadb/ca.crt

# TLS with full verification
mysql://dbpulse:secret@tcp(localhost:3306)/testdb?ssl-mode=VERIFY_IDENTITY&ssl-ca=.certs/mariadb/ca.crt
```

## Testing TLS Connections

### PostgreSQL

```bash
# Using psql
PGSSLMODE=require psql "postgresql://postgres:secret@localhost:5432/testdb" \
  -c "SELECT ssl_is_used(), version, cipher FROM pg_stat_ssl WHERE pid = pg_backend_pid();"

# Verify SSL is enabled
podman exec dbpulse-postgres-tls psql -U postgres -d testdb -c "SHOW ssl;"
```

### MariaDB

```bash
# Using mariadb client
mariadb -h 127.0.0.1 -u dbpulse -psecret -D testdb --ssl-mode=REQUIRED \
  -e "SHOW STATUS LIKE 'Ssl_cipher';"

# Verify SSL is enabled
podman exec dbpulse-mariadb-tls mariadb -u root -psecret -e "SHOW VARIABLES LIKE 'have_ssl';"
```

## GitHub Actions

The repository includes a dedicated workflow for TLS integration testing:

**`.github/workflows/integration-test-tls.yml`**
- Generates certificates on-the-fly
- Starts Docker containers with TLS enabled
- Runs integration tests with TLS connections
- Verifies TLS is actually being used
- Shows logs on failure

## Troubleshooting

### Certificate Issues

```bash
# Verify certificate validity
openssl verify -CAfile .certs/postgres/ca.crt .certs/postgres/server.crt
openssl verify -CAfile .certs/mariadb/ca.crt .certs/mariadb/server.crt

# Inspect certificate details
openssl x509 -in .certs/postgres/server.crt -text -noout

# Check certificate expiration
openssl x509 -in .certs/postgres/server.crt -noout -dates
```

#### PostgreSQL Key Permission Issues

PostgreSQL requires the private key to be owned by the `postgres` user and have `600` permissions. When using Docker bind mounts, this can be problematic. Solutions:

1. **Use Docker COPY (Recommended for CI/CD)**:
   ```bash
   # Build custom image with proper permissions
   cat > Dockerfile.postgres-tls <<EOF
   FROM postgres:17-alpine
   COPY .certs/postgres/server.crt /var/lib/postgresql/server.crt
   COPY .certs/postgres/server.key /var/lib/postgresql/server.key
   COPY .certs/postgres/ca.crt /var/lib/postgresql/ca.crt
   RUN chown postgres:postgres /var/lib/postgresql/server.* /var/lib/postgresql/ca.crt && \
       chmod 600 /var/lib/postgresql/server.key && \
       chmod 644 /var/lib/postgresql/server.crt /var/lib/postgresql/ca.crt
   EOF
   docker build -t postgres-tls:local -f Dockerfile.postgres-tls .
   docker run -d --name postgres-tls -p 5432:5432 postgres-tls:local -c ssl=on ...
   ```

2. **Fix MariaDB Key Permissions**:
   ```bash
   # MariaDB requires readable key when using bind mounts
   chmod 644 .certs/mariadb/server.key
   ```
```

### Container Issues

```bash
# Check container logs
podman logs dbpulse-postgres-tls
podman logs dbpulse-mariadb-tls

# Check container status
podman ps -a | grep dbpulse

# Clean up manually
podman rm -f dbpulse-postgres-tls dbpulse-mariadb-tls
rm -rf .certs
```

### Connection Issues

```bash
# Test PostgreSQL connectivity
podman exec dbpulse-postgres-tls pg_isready -U postgres

# Test MariaDB connectivity
podman exec dbpulse-mariadb-tls mariadb -u dbpulse -psecret -D testdb -e "SELECT 1"

# Check if ports are listening
ss -tlnp | grep -E '5432|3306'
```

## Best Practices

1. **Certificate Regeneration**: Run `just test-tls` - it regenerates certificates automatically
2. **Clean State**: The justfile handles cleanup automatically between test runs
3. **Port Conflicts**: Ensure ports 5432 and 3306 are available before running tests

## Environment Variables

- `TEST_POSTGRES_DSN`: Override PostgreSQL connection string for tests
- `TEST_MARIADB_DSN`: Override MariaDB connection string for tests

## References

- [PostgreSQL SSL Support](https://www.postgresql.org/docs/current/ssl-tcp.html)
- [MariaDB TLS Configuration](https://mariadb.com/kb/en/securing-connections-for-client-and-server/)
- [Podman](https://podman.io/)
