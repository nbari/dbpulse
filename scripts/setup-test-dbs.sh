#!/bin/bash
set -euo pipefail

# Setup local PostgreSQL and MariaDB for testing dbpulse
# This script ensures both databases are running and ready

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Container configuration
PG_CONTAINER="dbpulse-postgres"
MARIADB_CONTAINER="dbpulse-mariadb"
PG_PORT=5432
MARIADB_PORT=3306
MAX_WAIT=30

log_info() {
    echo -e "${GREEN}[INFO]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

log_debug() {
    echo -e "${BLUE}[DEBUG]${NC} $*"
}

# Check if podman is available
if ! command -v podman &>/dev/null; then
    log_error "podman is not installed or not in PATH"
    exit 1
fi

# Function to check if container is running
is_container_running() {
    local container_name=$1
    podman ps --format '{{.Names}}' 2>/dev/null | grep -q "^${container_name}$"
}

# Function to start PostgreSQL container
start_postgres() {
    if is_container_running "$PG_CONTAINER"; then
        log_info "PostgreSQL container already running"
        return 0
    fi

    log_info "Starting PostgreSQL container..."
    podman run --rm --name "$PG_CONTAINER" \
        -e POSTGRES_USER=postgres \
        -e POSTGRES_PASSWORD=secret \
        -e POSTGRES_DB=testdb \
        -p "${PG_PORT}:5432" \
        -d postgres:latest >/dev/null 2>&1

    if [ $? -eq 0 ]; then
        log_info "PostgreSQL container started"
    else
        log_error "Failed to start PostgreSQL container"
        return 1
    fi
}

# Function to start MariaDB container
start_mariadb() {
    if is_container_running "$MARIADB_CONTAINER"; then
        log_info "MariaDB container already running"
        return 0
    fi

    log_info "Starting MariaDB container..."
    podman run --rm --name "$MARIADB_CONTAINER" \
        -e MARIADB_USER=dbpulse \
        -e MARIADB_PASSWORD=secret \
        -e MARIADB_ROOT_PASSWORD=secret \
        -e MARIADB_DATABASE=testdb \
        -p "${MARIADB_PORT}:3306" \
        -d mariadb:latest >/dev/null 2>&1

    if [ $? -eq 0 ]; then
        log_info "MariaDB container started"
    else
        log_error "Failed to start MariaDB container"
        return 1
    fi
}

# Function to wait for PostgreSQL to be ready
wait_for_postgres() {
    log_info "Waiting for PostgreSQL to be ready..."
    local elapsed=0

    while [ "$elapsed" -lt "$MAX_WAIT" ]; do
        if podman exec "$PG_CONTAINER" pg_isready -U postgres >/dev/null 2>&1; then
            # Extra check: try to connect and run a simple query
            if podman exec "$PG_CONTAINER" psql -U postgres -d testdb -c "SELECT 1" >/dev/null 2>&1; then
                log_info "PostgreSQL is ready (${elapsed}s)"
                return 0
            fi
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done

    log_error "PostgreSQL failed to become ready after ${MAX_WAIT}s"
    return 1
}

# Function to wait for MariaDB to be ready
wait_for_mariadb() {
    log_info "Waiting for MariaDB to be ready..."
    local elapsed=0

    while [ "$elapsed" -lt "$MAX_WAIT" ]; do
        if podman exec "$MARIADB_CONTAINER" mariadb-admin ping -u root -psecret >/dev/null 2>&1; then
            # Extra check: try to connect and run a simple query
            if podman exec "$MARIADB_CONTAINER" mariadb -u dbpulse -psecret -D testdb -e "SELECT 1" >/dev/null 2>&1; then
                log_info "MariaDB is ready (${elapsed}s)"
                return 0
            fi
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done

    log_error "MariaDB failed to become ready after ${MAX_WAIT}s"
    return 1
}

# Function to verify database is accessible
verify_postgres() {
    log_info "Verifying PostgreSQL connection..."
    if podman exec "$PG_CONTAINER" psql -U postgres -d testdb -c "SELECT version();" >/dev/null 2>&1; then
        local version=$(podman exec "$PG_CONTAINER" psql -U postgres -d testdb -t -c "SELECT version();" | head -1 | xargs)
        log_info "PostgreSQL connection verified: ${version}"
        
        # Clean up test tables to ensure fresh start
        log_info "Cleaning PostgreSQL test tables..."
        podman exec "$PG_CONTAINER" psql -U postgres -d testdb -c "DROP TABLE IF EXISTS dbpulse_rw CASCADE;" > /dev/null 2>&1
        return 0
    else
        log_error "Failed to verify PostgreSQL connection"
        return 1
    fi
}

# Function to verify MariaDB is accessible
verify_mariadb() {
    log_info "Verifying MariaDB connection..."
    if podman exec "$MARIADB_CONTAINER" mariadb -u dbpulse -psecret -D testdb -e "SELECT VERSION();" >/dev/null 2>&1; then
        local version=$(podman exec "$MARIADB_CONTAINER" mariadb -u dbpulse -psecret -D testdb -s -N -e "SELECT VERSION();")
        log_info "MariaDB connection verified: ${version}"
        
        # Clean up test tables to ensure fresh start
        log_info "Cleaning MariaDB test tables..."
        podman exec "$MARIADB_CONTAINER" mariadb -u dbpulse -psecret -D testdb -e "DROP TABLE IF EXISTS dbpulse_rw;" > /dev/null 2>&1
        return 0
    else
        log_error "Failed to verify MariaDB connection"
        return 1
    fi
}

# Main execution
main() {
    log_info "Setting up test databases for dbpulse..."

    # Start containers if not running
    start_postgres || exit 1
    start_mariadb || exit 1

    # Wait for databases to be ready
    wait_for_postgres || exit 1
    wait_for_mariadb || exit 1

    # Verify connections
    verify_postgres || exit 1
    verify_mariadb || exit 1

    log_info "✅ All test databases are ready!"
    return 0
}

# Run main function
main "$@"
