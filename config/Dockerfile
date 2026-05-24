# syntax=docker/dockerfile:1.7

FROM rust:1.90-slim-bookworm AS builder

WORKDIR /app
ARG SYSLOG_BUILD_PROFILE=release
RUN case "$SYSLOG_BUILD_PROFILE" in release|debug) ;; *) echo "invalid SYSLOG_BUILD_PROFILE=$SYSLOG_BUILD_PROFILE"; exit 1; esac
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Cache deps
COPY Cargo.toml Cargo.lock ./
RUN --mount=type=cache,id=syslog-cargo-registry-v2,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=syslog-cargo-git-v2,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=syslog-cargo-target-v2,target=/app/target,sharing=locked \
    case "$SYSLOG_BUILD_PROFILE" in release|debug) ;; *) echo "invalid SYSLOG_BUILD_PROFILE=$SYSLOG_BUILD_PROFILE"; exit 1; esac && \
    mkdir src && echo "fn main() {}" > src/main.rs && \
    if [ "$SYSLOG_BUILD_PROFILE" = "release" ]; then cargo build --release --locked; else cargo build --locked; fi && \
    rm -rf src

# Build real binary
COPY src/ src/
COPY plugins/syslog/skills/syslog-frustration-assessment/ plugins/syslog/skills/syslog-frustration-assessment/
COPY docker-compose.yml docker-compose.prod.yml ./
COPY config/Dockerfile config/Dockerfile
RUN --mount=type=cache,id=syslog-cargo-registry-v2,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=syslog-cargo-git-v2,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=syslog-cargo-target-v2,target=/app/target,sharing=locked \
    case "$SYSLOG_BUILD_PROFILE" in release|debug) ;; *) echo "invalid SYSLOG_BUILD_PROFILE=$SYSLOG_BUILD_PROFILE"; exit 1; esac && \
    touch src/main.rs && \
    if [ "$SYSLOG_BUILD_PROFILE" = "release" ]; then \
      cargo build --release --locked && cp target/release/syslog /usr/local/bin/syslog; \
    else \
      cargo build --locked && cp target/debug/syslog /usr/local/bin/syslog; \
    fi

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
