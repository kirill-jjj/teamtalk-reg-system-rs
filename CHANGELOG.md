# Changelog

## [0.1.3] - 2026-01-26
### Added
- Admin panel pagination for TeamTalk accounts, users, and banlist.
- Database compatibility + strict migrations and follow-up defaults for `created_at`.
- Schema/integrity checks and richer DB error logging.
- Configurable log level in config.
### Fixed
- TeamTalk account existence checks now use the accounts list (not online users).
- Reduced TT worker list log noise and improved list handling.
- Corrected migration transaction behavior and checksum handling via follow-up migration.

## [0.1.2] - 2026-01-26
### Fixed
- Added explicit error logging for TG handlers, TT command dispatch, dialogue state reads, and web download flows.
- Improved logging around admin approval flows and DB sync notifications.
