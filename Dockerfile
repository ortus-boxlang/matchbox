FROM rust:1-bookworm AS builder

WORKDIR /usr/src/matchbox

COPY Cargo.toml Cargo.lock build.rs ./
COPY crates ./crates
COPY src ./src

RUN rustup target add wasm32-wasip1 wasm32-unknown-unknown \
    && cargo build --release --bin matchbox

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /usr/src/matchbox/target/release/matchbox /usr/local/bin/matchbox

EXPOSE 8080

ENTRYPOINT ["matchbox"]
CMD ["--help"]
