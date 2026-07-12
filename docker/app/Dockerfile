FROM rust:1.97-slim@sha256:14c4fe50ea427dc42381a1a09a9a839c1d2346a2e508cd491bf02c659dbc0ed7 AS builder
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
