FROM clux/muslrust
RUN mkdir /source
WORKDIR /source
COPY . .
RUN cargo build --release
RUN strip ./target/x86_64-unknown-linux-musl/release/dbpulse

FROM scratch
COPY --from=0 /source/target/x86_64-unknown-linux-musl/release/dbpulse /
CMD ["./dbpulse"]
