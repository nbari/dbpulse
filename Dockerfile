FROM rust:1.35
WORKDIR /usr/src/dbpulse
COPY . .
RUN cargo build --release

FROM debian:latest
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y openssl ca-certificates
COPY --from=0 /usr/src/dbpulse/target/release/dbpulse /
CMD ["./dbpulse"]
