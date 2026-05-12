FROM rust:bookworm AS builder

RUN apt-get update && apt-get install -y \
    libssl-dev \
    pkg-config \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

# Cap cargo's encoder/codegen parallelism on memory-constrained CI
# runners (defaults to one job per CPU, which OOMs the self-hosted
# runner). Override at build time with --build-arg CARGO_BUILD_JOBS=N.
# `default` is the literal cargo accepts to mean "one job per CPU";
# leaving the ARG empty would set CARGO_BUILD_JOBS="" which cargo
# rejects with "Number of parallel jobs should be `default` or a number".
ARG CARGO_BUILD_JOBS=default
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}

ENV SQLX_OFFLINE=true
RUN cargo build --release --locked

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    libssl3 \
    libpq5 \
    ca-certificates \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/fidget-backend .

EXPOSE 8080
CMD ["./fidget-backend"]
