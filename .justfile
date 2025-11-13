default: test
  @just --list

# Test suite (includes unit tests + integration tests)
test: clippy fmt unit-test
  @echo "✅ All tests passed!"

# Unit tests
unit-test:
  @echo "🧪 Running unit tests..."
  cargo test -- --nocapture

# Run tests with coverage
coverage:
  @echo "📊 Running tests with coverage..."
  cargo llvm-cov --all-features --workspace

# Linting
clippy:
  @echo "🔍 Running clippy..."
  cargo clippy --all-targets --all-features -- -D clippy::all -D clippy::nursery -D warnings

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
    echo "🔗 Monitor release: https://github.com/nbari/cron-when/actions"

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

# Check outdated dependencies
outdated:
  @echo "📅 Checking for outdated dependencies..."
  cargo outdated --root-deps-only

# Expand macros for debugging
expand:
  @echo "🔍 Expanding macros..."
  cargo expand

# Run example with sample crontab
example:
  @echo "📖 Running example with sample.crontab..."
  cargo run -- -f sample.crontab

# Start PostgreSQL container for integration testing
postgres:
  podman run --rm --name dbpulse-postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=secret \
  -e POSTGRES_DB=testdb \
  -p 5432:5432 \
  -d postgres:latest

# Start MariaDB container for integration testing
mariadb:
  podman run --rm --name dbpulse-mariadb \
  -e MARIADB_USER=dbpulse \
  -e MARIADB_PASSWORD=secret \
  -e MARIADB_ROOT_PASSWORD=secret \
  -e MARIADB_DATABASE=testdb \
  -p 3306:3306 \
  -d mariadb:latest

# Stop all test database containers
stop-db:
  -podman rm -f dbpulse-postgres dbpulse-mariadb 2>/dev/null || true
  @sleep 2

# Run integration tests against local databases (quick smoke test)
integration-test: build stop-db
  @echo "🚀 Starting integration smoke tests..."
  @just postgres
  @just mariadb
  @sleep 5
  @echo "📊 Testing PostgreSQL..."
  @timeout 30s cargo run --release -- --dsn "postgres://postgres:secret@tcp(localhost:5432)/testdb" --interval 5 --range 100 || true
  @echo "📊 Testing MariaDB..."
  @timeout 30s cargo run --release -- --dsn "mysql://dbpulse:secret@tcp(localhost:3306)/testdb" --interval 5 --range 100 || true
  @just stop-db
  @echo "✅ Integration smoke tests complete!"

# Run full integration test suite (requires running databases)
test-integration:
  @echo "🧪 Running integration tests..."
  @scripts/setup-test-dbs.sh || (echo "❌ Test database setup failed. Fix the issues above before running tests." && exit 1)
  @echo "Testing PostgreSQL..."
  cargo test --test postgres_test -- --ignored --nocapture
  @echo "Testing MariaDB..."
  cargo test --test mariadb_test -- --ignored --nocapture
  @just stop-db
  @echo "✅ Integration tests complete!"

# Run only PostgreSQL integration tests
test-postgres-integration:
  @echo "🧪 Running PostgreSQL integration tests..."
  @scripts/setup-test-dbs.sh || exit 1
  cargo test --test postgres_test -- --ignored --nocapture
  @just stop-db

# Run only MariaDB integration tests
test-mariadb-integration:
  @echo "🧪 Running MariaDB integration tests..."
  @scripts/setup-test-dbs.sh || exit 1
  cargo test --test mariadb_test -- --ignored --nocapture
  @just stop-db

build-container:
  docker build -t dbpulse .

rpm: build
  mkdir rpm; docker run -it --rm -v ./rpm:/host dbpulse
