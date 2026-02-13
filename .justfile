default: test
  @just --list

# Run all tests (unit, integration, TLS)
test: clippy fmt
  @echo "🧪 Running unit tests..."
  @cargo test --lib --bins
  @echo "🧪 Running integration tests..."
  @just test-integration
  @echo "🧪 Running TLS tests..."
  @just test-tls
  @echo "✅ All tests passed!"

# Run only unit tests
unit-test:
  @cargo test --lib --bins

# Run tests with coverage
coverage:
  @echo "📊 Running tests with coverage..."
  cargo llvm-cov --all-features --workspace

# Linting
clippy:
  @echo "🔍 Running clippy..."
  cargo clippy --all-targets --all-features

# Formatting
fmt:
  @echo "🎨 Formatting code..."
  cargo fmt --all

# Run benchmarks
bench:
  @echo "⚡ Running benchmarks..."
  cargo bench

# Build release version
build:
  @echo "🔨 Building release..."
  cargo build --release

# Build with musl for static linking
build-musl:
  @echo "🔨 Building with musl..."
  cargo build --release --features musl --target x86_64-unknown-linux-musl

# Update dependencies
update:
  @echo "⬆️  Updating dependencies..."
  cargo update

# Clean build artifacts
clean:
  @echo "🧹 Cleaning build artifacts..."
  cargo clean

# Get current version
version:
    @cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version'

# Check if working directory is clean
check-clean:
    #!/usr/bin/env bash
    if [[ -n $(git status --porcelain) ]]; then
        echo "❌ Working directory is not clean. Commit or stash your changes first."
        git status --short
        exit 1
    fi
    echo "✅ Working directory is clean"

# Check if on develop branch
check-develop:
    #!/usr/bin/env bash
    current_branch=$(git branch --show-current)
    if [[ "$current_branch" != "develop" ]]; then
        echo "❌ Not on develop branch (currently on: $current_branch)"
        echo "Switch to develop branch first: git checkout develop"
        exit 1
    fi
    echo "✅ On develop branch"

# Bump version and commit (patch level)
bump: check-develop check-clean update clean test
    #!/usr/bin/env bash
    echo "🔧 Bumping patch version..."
    cargo set-version --bump patch
    new_version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
    echo "📝 New version: $new_version"

    git add .
    git commit -m "bump version to $new_version"
    git push origin develop
    echo "✅ Version bumped and pushed to develop"

# Bump minor version
bump-minor: check-develop check-clean update clean test
    #!/usr/bin/env bash
    echo "🔧 Bumping minor version..."
    cargo set-version --bump minor
    new_version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
    echo "📝 New version: $new_version"

    git add .
    git commit -m "bump version to $new_version"
    git push origin develop
    echo "✅ Version bumped and pushed to develop"

# Bump major version
bump-major: check-develop check-clean update clean test
    #!/usr/bin/env bash
    echo "🔧 Bumping major version..."
    cargo set-version --bump major
    new_version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
    echo "📝 New version: $new_version"

    git add .
    git commit -m "bump version to $new_version"
    git push origin develop
    echo "✅ Version bumped and pushed to develop"

# Internal function to handle the merge and tag process
_deploy-merge-and-tag:
    #!/usr/bin/env bash
    set -euo pipefail

    new_version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
    echo "🚀 Starting deployment for version $new_version..."

    # Ensure develop is up to date
    echo "🔄 Ensuring develop is up to date..."
    git pull origin develop

    # Switch to main and merge develop
    echo "🔄 Switching to main branch..."
    git checkout main
    git pull origin main

    echo "🔀 Merging develop into main..."
    if ! git merge develop --no-edit; then
        echo "❌ Merge failed! Please resolve conflicts manually."
        git checkout develop
        exit 1
    fi

    # Create signed tag
    echo "🏷️  Creating signed tag $new_version..."
    git tag -s "$new_version" -m "Release version $new_version"

    # Push main and tag atomically
    echo "⬆️  Pushing main branch and tag..."
    if ! git push origin main "$new_version"; then
        echo "❌ Push failed! Rolling back..."
        git tag -d "$new_version"
        git checkout develop
        exit 1
    fi

    # Switch back to develop
    echo "🔄 Switching back to develop..."
    git checkout develop

    echo "✅ Deployment complete!"
    echo "🎉 Version $new_version has been released"
    echo "📋 Summary:"
    echo "   - develop branch: bumped and pushed"
    echo "   - main branch: merged and pushed"
    echo "   - tag $new_version: created and pushed"
    echo "🔗 Monitor release: https://github.com/nbari/dbpulse/actions"

# Deploy: merge to main, tag, and push everything
deploy: bump _deploy-merge-and-tag

# Deploy with minor version bump
deploy-minor: bump-minor _deploy-merge-and-tag

# Deploy with major version bump
deploy-major: bump-major _deploy-merge-and-tag

# Create & push a test tag like t-YYYYMMDD-HHMMSS (skips publish/release in CI)
# Usage:
#   just t-deploy
#   just t-deploy "optional tag message"
t-deploy message="CI test": check-develop check-clean test
    #!/usr/bin/env bash
    set -euo pipefail

    TAG_MESSAGE="{{message}}"
    ts="$(date -u +%Y%m%d-%H%M%S)"
    tag="t-${ts}"

    echo "🏷️  Creating signed test tag: ${tag}"
    git fetch --tags --quiet

    if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
        echo "❌ Tag ${tag} already exists. Aborting." >&2
        exit 1
    fi

    git tag -s "${tag}" -m "${TAG_MESSAGE}"
    git push origin "${tag}"

    echo "✅ Pushed ${tag}"
    echo "🧹 To remove it:"
    echo "   git push origin :refs/tags/${tag} && git tag -d ${tag}"

# Full CI check (what runs in CI)
ci: clippy fmt test
  @echo "✅ All CI checks passed!"

# Run integration tests (non-TLS)
test-integration:
  #!/usr/bin/env bash
  set -e
  echo "🧪 Running integration tests..."

  # Clean up any existing containers first
  podman rm -f dbpulse-postgres dbpulse-mariadb dbpulse-postgres-tls dbpulse-mariadb-tls 2>/dev/null || true

  # Start databases
  podman run -d --name dbpulse-postgres \
    -e POSTGRES_USER=postgres -e POSTGRES_PASSWORD=secret -e POSTGRES_DB=testdb \
    -p 5432:5432 postgres:latest

  podman run -d --name dbpulse-mariadb \
    -e MARIADB_USER=dbpulse -e MARIADB_PASSWORD=secret \
    -e MARIADB_ROOT_PASSWORD=secret -e MARIADB_DATABASE=testdb \
    -p 3306:3306 mariadb:latest

  echo "⏳ Waiting for databases to be ready..."

  # Wait for PostgreSQL
  for i in {1..30}; do
    if podman exec dbpulse-postgres pg_isready -U postgres > /dev/null 2>&1; then
      echo "✓ PostgreSQL ready"
      break
    fi
    sleep 1
  done

  # Wait for MariaDB
  for i in {1..30}; do
    if podman exec dbpulse-mariadb mariadb -u dbpulse -psecret -D testdb -e "SELECT 1" > /dev/null 2>&1; then
      echo "✓ MariaDB ready"
      break
    fi
    sleep 1
  done

  # Run tests
  if ! cargo test --test postgres_test -- --ignored --nocapture; then
    echo "❌ PostgreSQL integration tests failed"
    podman rm -f dbpulse-postgres dbpulse-mariadb > /dev/null 2>&1
    exit 1
  fi

  if ! cargo test --test mariadb_test -- --ignored --nocapture; then
    echo "❌ MariaDB integration tests failed"
    podman rm -f dbpulse-postgres dbpulse-mariadb > /dev/null 2>&1
    exit 1
  fi

  # Cleanup
  podman rm -f dbpulse-postgres dbpulse-mariadb > /dev/null 2>&1
  echo "✅ Integration tests complete!"

# ===== TLS Testing =====

# Run all TLS integration tests (setup, test, cleanup)
test-tls:
  #!/usr/bin/env bash
  set -e

  echo "🔐 Setting up TLS testing environment..."

  # Clean up any existing containers first
  podman rm -f dbpulse-postgres dbpulse-mariadb dbpulse-postgres-tls dbpulse-mariadb-tls 2>/dev/null || true

  ./scripts/gen-certs.sh > /dev/null 2>&1
  chmod 644 .certs/mariadb/server.key

  # Build PostgreSQL image with proper key permissions
  cat > Dockerfile.postgres-tls <<'EOF'
  FROM postgres:17-alpine
  COPY .certs/postgres/server.crt /var/lib/postgresql/server.crt
  COPY .certs/postgres/server.key /var/lib/postgresql/server.key
  COPY .certs/postgres/ca.crt /var/lib/postgresql/ca.crt
  RUN chown postgres:postgres /var/lib/postgresql/server.* /var/lib/postgresql/ca.crt && \
      chmod 600 /var/lib/postgresql/server.key && \
      chmod 644 /var/lib/postgresql/server.crt /var/lib/postgresql/ca.crt
  EOF

  echo "🚀 Starting TLS-enabled databases..."
  podman build -t postgres-tls:test -f Dockerfile.postgres-tls . > /dev/null 2>&1

  podman run -d --name dbpulse-postgres-tls \
    -e POSTGRES_USER=postgres \
    -e POSTGRES_PASSWORD=secret \
    -e POSTGRES_DB=testdb \
    -p 5432:5432 \
    postgres-tls:test \
    -c ssl=on \
    -c ssl_cert_file=/var/lib/postgresql/server.crt \
    -c ssl_key_file=/var/lib/postgresql/server.key \
    -c ssl_ca_file=/var/lib/postgresql/ca.crt \
    -c ssl_min_protocol_version=TLSv1.2

  podman run -d --name dbpulse-mariadb-tls \
    -e MARIADB_USER=dbpulse \
    -e MARIADB_PASSWORD=secret \
    -e MARIADB_ROOT_PASSWORD=secret \
    -e MARIADB_DATABASE=testdb \
    -p 3306:3306 \
    -v $(pwd)/.certs/mariadb/server.crt:/etc/mysql/ssl/server.crt:ro \
    -v $(pwd)/.certs/mariadb/server.key:/etc/mysql/ssl/server.key:ro \
    -v $(pwd)/.certs/mariadb/ca.crt:/etc/mysql/ssl/ca.crt:ro \
    mariadb:11 \
    --ssl-cert=/etc/mysql/ssl/server.crt \
    --ssl-key=/etc/mysql/ssl/server.key \
    --ssl-ca=/etc/mysql/ssl/ca.crt \
    --require-secure-transport=OFF \
    --tls-version=TLSv1.2,TLSv1.3

  echo "⏳ Waiting for databases to be ready..."

  # Wait for PostgreSQL
  for i in {1..30}; do
    if podman exec dbpulse-postgres-tls pg_isready -U postgres > /dev/null 2>&1; then
      echo "✓ PostgreSQL ready"
      break
    fi
    if [ $i -eq 30 ]; then
      echo "❌ PostgreSQL failed to start"
      podman logs dbpulse-postgres-tls
      podman rm -f dbpulse-postgres-tls dbpulse-mariadb-tls > /dev/null 2>&1
      exit 1
    fi
    sleep 1
  done

  # Wait for MariaDB
  for i in {1..30}; do
    if podman exec dbpulse-mariadb-tls mariadb -u dbpulse -psecret -D testdb -e "SELECT 1" > /dev/null 2>&1; then
      echo "✓ MariaDB ready"
      break
    fi
    if [ $i -eq 30 ]; then
      echo "❌ MariaDB failed to start"
      podman logs dbpulse-mariadb-tls
      podman rm -f dbpulse-postgres-tls dbpulse-mariadb-tls > /dev/null 2>&1
      exit 1
    fi
    sleep 1
  done

  echo "🧪 Running TLS integration tests..."
  if ! cargo test --test postgres_tls_test -- --ignored --nocapture; then
    echo "❌ PostgreSQL TLS tests failed"
    podman rm -f dbpulse-postgres-tls dbpulse-mariadb-tls > /dev/null 2>&1
    rm -f Dockerfile.postgres-tls
    exit 1
  fi

  if ! cargo test --test mariadb_tls_test -- --ignored --nocapture; then
    echo "❌ MariaDB TLS tests failed"
    podman rm -f dbpulse-postgres-tls dbpulse-mariadb-tls > /dev/null 2>&1
    rm -f Dockerfile.postgres-tls
    exit 1
  fi

  echo "🧹 Cleaning up..."
  podman rm -f dbpulse-postgres-tls dbpulse-mariadb-tls > /dev/null 2>&1
  rm -f Dockerfile.postgres-tls
  echo "✅ All TLS tests passed!"
