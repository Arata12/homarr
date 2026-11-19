# Migrations

Applied automatically at startup by `homarr-db` (sqlx embedded migrator, tracked in `_sqlx_migrations`).

- Add the next change as `NNNN_description.sql` (e.g. `0002_add_foo.sql`) in BOTH `sqlite/` and `postgres/`, using the same number.
- Never edit or rename an applied file (sqlx verifies checksums); write a new migration instead.
- `0001_initial.sql` is idempotent so databases created before tracking existed are adopted by re-running it once.
- Files are embedded at compile time; rebuild after adding one.
