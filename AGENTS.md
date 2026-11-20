# Homarr (Rust edition) — Agent Rules

This repository is a fork of [homarr-labs/homarr](https://github.com/homarr-labs/homarr) rewritten as a pure Rust
workspace (v3.x). The upstream TypeScript monorepo (Next.js, tRPC, Bun, Turborepo, Mantine) no longer exists here;
ignore any upstream docs, skills or habits that assume it.

## Git workflow (mandatory)

- Work directly on `dev` (the default branch). Do not create feature or session branches unless the owner asks.
- The owner works from a phone: they cannot merge, tag or delete branches by hand. Finish the whole flow yourself.
- Commit with conventional commits (`feat:`, `fix:`, `chore:` …) and push to `dev`.

## Releases

1. Bump `[workspace.package] version` in the root `Cargo.toml` (crates inherit it with `version.workspace = true`)
   and refresh the lockfile with `cargo update --workspace`. Commit as `chore(release): vX.Y.Z` on `dev`.
2. Push a `vX.Y.Z` tag. If tag pushes are blocked, run the `Release` workflow (`release.yml`) via
   `workflow_dispatch` with input `tag=vX.Y.Z` on `dev`: it creates the release and the tag on the `dev` head, so
   the bump commit must already be there.
3. The workflow builds static musl binaries (amd64, arm64), attaches them to the release and publishes
   `ghcr.io/arata12/homarr:<version>` plus `latest` (stable releases only).

Semver: `fix:`-only changes → patch; new features → minor; breaking config/data changes → major.

## Repository structure

```
crates/
├── homarr-server/        # Main binary: axum server wiring API, web UI, static assets, background tasks
├── homarr-cli/           # `homarr` ops CLI binary
├── homarr-workshop/      # Workshop service binary
├── homarr-api/           # JSON API routes, OpenAPI, MCP endpoint, WebSocket, CSRF, AppState
├── homarr-web/           # Server-rendered HTML pages (board, editor, manage, auth) + i18n
├── homarr-auth/          # Sessions, passwords, API keys, permission checks, axum extractors
├── homarr-db/            # sqlx pool (SQLite/PostgreSQL), models, repositories, seed, backup
├── homarr-core/          # Env config (AppConfig), logging, HTTP client factory, trusted certificates
├── homarr-common/        # Shared errors, IDs, encryption, URL helpers, grid algorithm, superjson
├── homarr-definitions/   # Domain enums/metadata: IntegrationKind, WidgetKind, permissions, settings
├── homarr-integrations/  # Integration clients grouped by domain (media, downloads_network, home_misc, stats, mock)
├── homarr-services/      # Docker, Kubernetes, icons, image proxy, ping, custom widgets, assistant
├── homarr-widgets/       # Widget registry, option schemas/sanitizing, rendering
├── homarr-tasks/         # Background/cron jobs
└── homarr-redis/         # Redis store (with in-memory fallback when REDIS_DISABLED)
assets/                   # locales/*.json, static/ (homarr.js, board-editor.js, homarr.css), public/
migrations/{sqlite,postgres}/  # sqlx migrations, embedded at compile time (see migrations/README.md)
docker/                   # Dockerfile.dev / .release / .workshop, entrypoint
.github/workflows/        # ci.yml (fmt, clippy, tests), release.yml (binaries + GHCR images)
```

## Commands

- `cargo fmt --all` — format (CI runs `cargo fmt --all -- --check`)
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — lint; warnings fail CI
- `cargo test --workspace --locked` — tests
- `cargo run -p homarr-server` — run the app locally (port 7575 by default; config via env, see `.env.example`)
- `docker compose --profile dev up` — dev container plus Redis

## Code rules

- Keep logic in the deepest crate that owns it; search `homarr-common` before adding a helper.
- New integration: add the kind and metadata in `homarr-definitions`, the client in the matching
  `homarr-integrations/src/groups/<domain>/` module, wire it into that group's `mod.rs` dispatch, and declare its
  auth in `http_auth.rs`. Reuse an existing widget when it covers the use case.
- New widget: add the `WidgetKind` in `homarr-definitions` and its metadata/options in `homarr-widgets`.
- Schema change: add a new numbered migration in BOTH `migrations/sqlite` and `migrations/postgres`. Never edit an
  applied migration.
- HTML in `homarr-web` is built from strings: every interpolated value must go through `html_escape`
  (`encode_text` / `encode_double_quoted_attribute`). Never insert user or integration data unescaped.
- Outbound HTTP must use `HttpClientFactory` from `homarr-core` (trusted certificates; `guarded()` /
  `guarded_get` block private-network SSRF targets for user-supplied URLs). Do not build ad-hoc `reqwest` clients.
- Secrets (integration credentials) are stored encrypted via `homarr-common` encryption; never log or return them.

## Tests and docs

- Add tests only when they plausibly prevent a regression in user-visible behavior or a security boundary.
- For focused changes run the affected crate (`cargo test -p <crate>`); run the full CI trio before releasing.
- Document only what users need to install, configure or troubleshoot (README, `.env.example`). A code change alone
  is not a reason to add docs.
