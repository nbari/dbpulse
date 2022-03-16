FROM rust:latest
WORKDIR /usr/src/dbpulse
COPY . .
RUN cargo build --release --locked

FROM debian:latest
COPY --from=0 /usr/src/dbpulse/target/release/dbpulse /
CMD ["./dbpulse"]
