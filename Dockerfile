FROM rust:1.35

WORKDIR /usr/src/dbpulse
COPY . .

RUN cargo install --path .
RUN rm -rf ./target

CMD ["dbpulse"]
