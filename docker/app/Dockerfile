FROM rust:1.91-slim@sha256:f75071363e7f4771769d4cf81b1b7b290e607f4d4459e8731f6abdcee9982dc8 AS builder
WORKDIR /build
COPY . .
RUN cargo build --release -p bridge-server

FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818
RUN apt-get update \
    && apt-get upgrade -y \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN groupadd -g 1000 app && useradd -u 1000 -g app -s /usr/sbin/nologin app
COPY --from=builder /build/target/release/bridge-server /usr/local/bin/bridge-server
WORKDIR /app
ENV CONFIG_PATH=/app/config.toml
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1
USER app:app
EXPOSE 8080
CMD ["bridge-server"]
