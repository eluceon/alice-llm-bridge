FROM rust:1.91-slim AS builder
WORKDIR /build
COPY . .
RUN cargo build --release -p bridge-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/bridge-server /usr/local/bin/bridge-server
WORKDIR /app
ENV CONFIG_PATH=/app/config.toml
EXPOSE 8080
CMD ["bridge-server"]
