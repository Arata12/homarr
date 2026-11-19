# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder
WORKDIR /workspace

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config sqlite3 \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
COPY migrations ./migrations

RUN cargo build --release --bin homarr-server --bin homarr

FROM debian:bookworm-slim AS cli
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /workspace/target/release/homarr /usr/local/bin/homarr
ENTRYPOINT ["/usr/local/bin/homarr"]

FROM debian:bookworm-slim AS runner
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /appdata/db /appdata/trusted-certificates /app/assets

WORKDIR /app
COPY --from=builder /workspace/target/release/homarr-server /usr/local/bin/homarr-server
COPY --from=builder /workspace/target/release/homarr /usr/local/bin/homarr
COPY assets ./assets

ENV HOST=0.0.0.0 \
    PORT=7575 \
    ASSETS_DIR=/app/assets \
    STATIC_DATA_DIR=/appdata \
    DB_DRIVER=sqlite \
    DB_URL=/appdata/db/db.sqlite

EXPOSE 7575
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=5 \
  CMD curl -fsS http://127.0.0.1:7575/api/health/live || exit 1

CMD ["/usr/local/bin/homarr-server"]
