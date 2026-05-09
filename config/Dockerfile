# syntax=docker/dockerfile:1.7

FROM rust:1.90-slim-bookworm AS builder

WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Cache deps
COPY Cargo.toml Cargo.lock ./
RUN --mount=type=cache,id=syslog-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=syslog-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=syslog-cargo-target,target=/app/target,sharing=locked \
    mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release --locked && rm -rf src

# Build real binary
COPY src/ src/
RUN --mount=type=cache,id=syslog-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=syslog-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=syslog-cargo-target,target=/app/target,sharing=locked \
    touch src/main.rs && cargo build --release --locked && cp target/release/syslog /usr/local/bin/syslog

# Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/syslog /usr/local/bin/syslog

RUN groupadd --gid 1000 syslog && useradd --uid 1000 --gid syslog --no-create-home --shell /sbin/nologin syslog && mkdir -p /data && chown syslog:syslog /data

ENV RUST_LOG=info
ENV SYSLOG_MCP_DB_PATH=/data/syslog.db

USER 1000:1000

EXPOSE 1514/udp 1514/tcp 3100/tcp

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -sf http://localhost:3100/health || exit 1

LABEL io.modelcontextprotocol.server.name="tv.tootie/syslog-mcp"

CMD ["syslog", "serve", "mcp"]
