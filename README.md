# Homarr v3.0 (Pure Rust Edition)

Homarr migrated into a modular 15-crate Rust workspace compiled, linted, tested, and run via Docker Compose.

## Workspace Crates

- `homarr-common` — IDs, AES-256-CBC/HMAC encryption, SuperJSON compat, responsive grid algorithm, outbound URL SSRF guard, formatters
- `homarr-definitions` — 67 `WidgetKind`s, 92 `IntegrationKind`s, RBAC permission matrix, board/section normalization, Docker discovery matchers
- `homarr-core` — Environment config (`AppConfig`), `LogStore`, `CertificateManager`, `HttpClientFactory`
- `homarr-db` — Dual SQLite & PostgreSQL (`sqlx`) 41-table migrations, repositories, default/demo seeding, JSON backup export/import
- `homarr-redis` — Live Redis (`redis`) + in-memory fallback, pub/sub channels, ping URL sets
- `homarr-auth` — Bcrypt authentication, cookie sessions, API keys, board/integration RBAC & Axum extractors
- `homarr-integrations` — Integration registry across all 92 service kinds + HTTP auth + mock telemetry
- `homarr-services` — Ping, HKDF/AES Image Proxy, Icon repository sync, Docker (`bollard`), Kubernetes, Custom Widgets v2 validator/runner, Open-Meteo/RSS/Stocks/Releases/Minecraft/DDG bangs, AI Assistant
- `homarr-tasks` — Tokio cron scheduler with DB overrides & Redis status broadcasts
- `homarr-widgets` — Metadata catalog, live data resolver, and XSS-safe HTML renderer for all 67 widget kinds
- `homarr-api` — 50+ OpenAPI 3.1 REST endpoints, WebSocket `/websockets` stream, MCP JSON-RPC 2.0 (`/api/mcp`) + OAuth
- `homarr-web` — SSR Mantine-inspired Web UI with 36 locales, responsive boards, Spotlight (`Ctrl+K`), onboarding, and `/manage/*`
- `homarr-server` — Production HTTP/WS server binary (port 7575)
- `homarr-cli` — Administration CLI binary (`homarr`)
- `homarr-workshop` — Standalone Custom Widget Workshop service (port 8090)

## Docker Compose Commands

```bash
# Run development toolchain (fmt, clippy, workspace tests) inside Docker
docker compose --profile postgres up -d redis postgres
docker compose --profile dev run --rm rust-dev cargo fmt --all -- --check
docker compose --profile dev run --rm rust-dev cargo clippy --workspace --all-targets -- -D warnings
docker compose --profile dev run --rm rust-dev cargo test --workspace

# Build & run production containers
docker compose build homarr cli
docker compose --profile workshop build workshop
DEMO_MODE=true DEMO_READ_ONLY=false docker compose --profile workshop up -d redis homarr workshop

# Run admin CLI
docker compose --profile cli run --rm cli users list
```
