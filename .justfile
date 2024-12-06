test: clippy
  cargo test

clippy:
  cargo clippy --all -- -W clippy::all -W clippy::nursery -D warnings

postgres:
  podman run --rm --name dbpulse-postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=secret \
  -p 5432:5432 \
  -d postgres:16 postgres

mariadb:
  podman run --rm --name dbpulse-mariadb \
  -e MARIADB_USER=root \
  -e MARIADB_ROOT_PASSWORD=secret \
  -p 3306:3306 \
  -d mariadb:latest

build:
  podman build -t dbpulse .
