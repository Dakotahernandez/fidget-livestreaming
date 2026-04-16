FROM rust:latest AS builder

RUN apt-get update && apt-get install -y \
    libssl-dev \
    pkg-config \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

ENV SQLX_OFFLINE=true
RUN cargo build --release

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
