#!/usr/bin/env bash
# Generate self-signed certificates for PostgreSQL and MariaDB testing

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CERTS_DIR="${SCRIPT_DIR}/../.certs"

# Create certificates directory
mkdir -p "${CERTS_DIR}"/{postgres,mariadb}

echo "Generating certificates for database integration tests..."

# Generate PostgreSQL certificates
echo "==> Generating PostgreSQL certificates..."
cd "${CERTS_DIR}/postgres"

# CA certificate
openssl req -new -x509 -days 3650 -nodes \
    -out ca.crt \
    -keyout ca.key \
    -subj "/CN=PostgreSQL CA"

# Server certificate
openssl req -new -nodes \
    -out server.csr \
    -keyout server.key \
    -subj "/CN=localhost"

openssl x509 -req -days 3650 \
    -in server.csr \
    -CA ca.crt \
    -CAkey ca.key \
    -CAcreateserial \
    -out server.crt \
    -extfile <(printf "subjectAltName=DNS:localhost,DNS:postgres,IP:127.0.0.1")

# Set proper permissions for PostgreSQL
chmod 600 server.key ca.key
chmod 644 server.crt ca.crt

# Client certificate (optional, for mutual TLS testing)
openssl req -new -nodes \
    -out client.csr \
    -keyout client.key \
    -subj "/CN=postgres"

openssl x509 -req -days 3650 \
    -in client.csr \
    -CA ca.crt \
    -CAkey ca.key \
    -CAcreateserial \
    -out client.crt

chmod 600 client.key
chmod 644 client.crt

# Clean up CSRs
rm -f *.csr *.srl

echo "✓ PostgreSQL certificates generated"

# Generate MariaDB certificates
echo "==> Generating MariaDB certificates..."
cd "${CERTS_DIR}/mariadb"

# CA certificate
openssl req -new -x509 -days 3650 -nodes \
    -out ca.crt \
    -keyout ca.key \
    -subj "/CN=MariaDB CA"

# Server certificate
openssl req -new -nodes \
    -out server.csr \
    -keyout server.key \
    -subj "/CN=localhost"

openssl x509 -req -days 3650 \
    -in server.csr \
    -CA ca.crt \
    -CAkey ca.key \
    -CAcreateserial \
    -out server.crt \
    -extfile <(printf "subjectAltName=DNS:localhost,DNS:mariadb,IP:127.0.0.1")

# Set proper permissions
chmod 644 *.crt
chmod 600 *.key

# Client certificate (optional)
openssl req -new -nodes \
    -out client.csr \
    -keyout client.key \
    -subj "/CN=dbpulse"

openssl x509 -req -days 3650 \
    -in client.csr \
    -CA ca.crt \
    -CAkey ca.key \
    -CAcreateserial \
    -out client.crt

chmod 644 client.crt
chmod 600 client.key

# Clean up CSRs
rm -f *.csr *.srl

echo "✓ MariaDB certificates generated"

echo ""
echo "Certificates generated successfully in: ${CERTS_DIR}"
echo ""
echo "To use with Podman Compose:"
echo "  podman-compose -f scripts/docker-compose-tls.yml up -d"
echo ""
echo "Or with Docker:"
echo "  docker compose -f scripts/docker-compose-tls.yml up -d"
echo ""
echo "To verify certificates:"
echo "  PostgreSQL: openssl verify -CAfile ${CERTS_DIR}/postgres/ca.crt ${CERTS_DIR}/postgres/server.crt"
echo "  MariaDB:    openssl verify -CAfile ${CERTS_DIR}/mariadb/ca.crt ${CERTS_DIR}/mariadb/server.crt"
