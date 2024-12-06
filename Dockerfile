FROM messense/rust-musl-cross:x86_64-musl AS builder

RUN apt-get update && apt-get install -y \
    git \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Set the working directory
WORKDIR /app

RUN git clone https://github.com/nbari/dbpulse.git .

RUN cargo build --release --locked --features "openssl/vendored"

FROM rust:latest

WORKDIR /app

RUN git clone https://github.com/nbari/dbpulse.git .

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/dbpulse /app/target/release/dbpulse

RUN cargo install cargo-generate-rpm

RUN strip -s /app/target/release/dbpulse

RUN cargo generate-rpm

ENTRYPOINT ["bash"]
