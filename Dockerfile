# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder
WORKDIR /workspace

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config sqlite3 \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
COPY migrations ./migrations

RUN cargo build --release --locked --bin homarr-server --bin homarr

FROM debian:bookworm-slim AS cli
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
ENV STATIC_DATA_DIR=/appdata \
    DB_DRIVER=sqlite \
    DB_URL=/appdata/db/db.sqlite
COPY --from=builder /workspace/target/release/homarr /usr/local/bin/homarr
# Same non-root user as the runner so files it creates in the shared /appdata stay
# writable by the server; use `docker compose run --user <PUID>:<PGID>` if you set PUID/PGID.
RUN groupadd -g 1000 homarr && useradd -u 1000 -g 1000 -M -d /appdata -s /usr/sbin/nologin homarr
USER 1000:1000
ENTRYPOINT ["/usr/local/bin/homarr"]

FROM debian:bookworm-slim AS runner
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && command -v setpriv \
    && groupadd -g 1000 homarr \
    && useradd -u 1000 -g 1000 -M -d /appdata -s /usr/sbin/nologin homarr \
    && mkdir -p /appdata/db /appdata/secrets /appdata/trusted-certificates /app/assets \
    && chown -R 1000:1000 /appdata

WORKDIR /app
COPY --from=builder /workspace/target/release/homarr-server /usr/local/bin/homarr-server
COPY --from=builder /workspace/target/release/homarr /usr/local/bin/homarr
COPY assets ./assets
COPY --chmod=0755 docker/entrypoint.sh /usr/local/bin/entrypoint.sh

ENV HOST=0.0.0.0 \
    PORT=7575 \
    ASSETS_DIR=/app/assets \
    STATIC_DATA_DIR=/appdata \
    DB_DRIVER=sqlite \
    DB_URL=/appdata/db/db.sqlite

EXPOSE 7575
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=5 \
  CMD ["/usr/local/bin/homarr", "healthcheck"]

# Starts as root only to chown /appdata, then drops to PUID:PGID (default 1000:1000)
# via setpriv. Set PUID=0 and PGID=0 to keep running as root.
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["/usr/local/bin/homarr-server"]
