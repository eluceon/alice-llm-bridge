FROM rust:1.98-slim@sha256:17d1ba895198f9934c6314ec5346a0d5115372f3243390c3d731e242f35c2f27 AS builder
WORKDIR /build
COPY . .
RUN cargo build --release -p bridge-server

FROM debian:bookworm-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df
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
