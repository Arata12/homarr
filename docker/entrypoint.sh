#!/bin/sh
# Entrypoint for the Debian runner image (Dockerfile target `runner`).
# Starts as root only long enough to fix /appdata ownership, then drops to
# PUID:PGID (default 1000:1000, the `homarr` user baked into the image).
# SECRET_ENCRYPTION_KEY_FILE / AUTH_SECRET_FILE are read by the server itself.
set -e

PUID=${PUID:-1000}
PGID=${PGID:-1000}

# Already non-root (e.g. compose `user:`): nothing we are allowed to change.
if [ "$(id -u)" != "0" ]; then
    exec "$@"
fi

# Explicit opt-in to run as root.
if [ "$PUID" = "0" ] && [ "$PGID" = "0" ]; then
    exec "$@"
fi

echo "Starting with UID='$PUID', GID='$PGID'"
# Only the small trees the server writes to (sqlite db, generated secrets,
# trusted certificates); the rest of /appdata and /app stays as is.
mkdir -p /appdata/db /appdata/secrets /appdata/trusted-certificates
chown "$PUID:$PGID" /appdata
chown -R "$PUID:$PGID" /appdata/db /appdata/secrets /appdata/trusted-certificates

# Keep supplementary groups from `group_add` (e.g. the docker.sock GID), minus root's group 0.
groups=$(id -G | tr ' ' '\n' | grep -vx 0 | paste -s -d, -)
if [ -n "$groups" ]; then
    exec setpriv --reuid="$PUID" --regid="$PGID" --groups="$groups" "$@"
fi
exec setpriv --reuid="$PUID" --regid="$PGID" --clear-groups "$@"
