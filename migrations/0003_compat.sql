-- Compatibility migration for databases created by the Python version.
-- Adds missing tables and indexes without tightening constraints.

CREATE TABLE IF NOT EXISTS pending_web_registrations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_key TEXT UNIQUE,
    username TEXT,
    password_cleartext TEXT,
    nickname TEXT,
    ip_address TEXT,
    user_agent TEXT,
    source_info TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS ix_pending_web_registrations_request_key
    ON pending_web_registrations(request_key);
CREATE INDEX IF NOT EXISTS ix_pending_web_registrations_username
    ON pending_web_registrations(username);
CREATE INDEX IF NOT EXISTS ix_pending_web_registrations_created_at
    ON pending_web_registrations(created_at);
CREATE INDEX IF NOT EXISTS ix_pending_web_registrations_id
    ON pending_web_registrations(id);
