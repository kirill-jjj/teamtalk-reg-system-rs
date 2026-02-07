# TeamTalk Registration System (Rust)

[![Build Binaries](https://github.com/kirill-jjj/teamtalk-reg-system-rs/actions/workflows/build.yml/badge.svg)](https://github.com/kirill-jjj/teamtalk-reg-system-rs/actions/workflows/build.yml)

Service for handling TeamTalk registrations via Telegram bot and web endpoints.

## Requirements

- Rust (stable)
- SQLite (via `sqlx` with the bundled driver)
- `sqlx-cli` installed (`cargo install sqlx-cli`)
- Optional: `lefthook` if you want hooks

## Quick Start

1) Create config and env files:

```bash
cp config.toml.example config.toml
```

Create `.env` with a database URL (example for local SQLite):

```bash
DATABASE_URL=sqlite:///absolute/path/to/db/dev.db
```

2) Create the database and apply migrations:

```bash
sqlx database create
sqlx migrate run
```

3) (Optional) Prepare SQLx query cache:

```bash
cargo sqlx prepare
```

4) Build and run:

```bash
cargo build
cargo run
```

## Configuration

The main configuration file is `config.toml`. Start from `config.toml.example`
and adjust values:

- Telegram bot token and admin IDs
- Host/port settings
- Registration policy toggles
- TeamTalk and download settings
- `pending_password_encryption_key` (Base64-encoded 32-byte key)
- Optional `log_level` (tracing filter), for example:
  - `log_level = "info"`
  - `log_level = "info,teamtalk_reg_system_rs=debug,teloxide=debug"`

Environment variables:

- `DATABASE_URL` is required by `sqlx` (used by the app and `cargo sqlx prepare`)

## Development

Run checks:

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
```

Git hooks (lefthook):

```bash
lefthook install
lefthook run pre-commit
```

## Notes

- The SQLite database file path must be absolute in `DATABASE_URL`.
- `cargo sqlx prepare` updates the `.sqlx/` query cache and should be committed.
- App startup fails fast when `pending_password_encryption_key` is missing or invalid.
