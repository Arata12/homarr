# Changelog

Release notes for each version, with binaries and Docker images, are on the
[GitHub releases page](https://github.com/Arata12/homarr/releases). For upstream Homarr
history (v2.x and earlier), see [homarr-labs/homarr](https://github.com/homarr-labs/homarr/releases).

## v3.0.2

### Fixed

- Boards load much faster: widget data is cached with upstream TTLs and resolved concurrently (#6).
- Container sections (categories) render nested with their title, collapse state per user and "open all" (#6).
- App status dot falls back to the app link when no ping URL is set (#6).
- Uploaded icons from upstream data load again via `/api/user-medias/{id}` (#6).
- The header shows the board's page title and logo (#6).
- Closed remaining gaps versus upstream: Docker endpoint TLS, session expiry, board access, validation and image proxy (#5).

## v3.0.1

### Fixed

- Security: closed critical and high-severity gaps versus upstream Homarr (#3).
- Security: closed remaining medium and low-severity gaps versus upstream Homarr (#4).

## v3.0.0

### Changed

- **Breaking:** Homarr rewritten as a pure Rust workspace (15 crates) replacing the TypeScript/Next.js stack.
- Static musl binaries for amd64 and arm64, and a distroless multi-arch image at `ghcr.io/arata12/homarr`.
- SQLite (default) and PostgreSQL support with migrations applied automatically at startup.
- Security hardening for authentication, SSRF protection and registration (#2).
