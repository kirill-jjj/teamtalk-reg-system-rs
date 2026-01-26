-- Add DEFAULT CURRENT_TIMESTAMP for created_at columns in strict tables.
-- SQLite requires table rebuild for altering defaults.

CREATE TABLE pending_telegram_registrations_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    request_key TEXT NOT NULL UNIQUE,
    registrant_telegram_id INTEGER NOT NULL,
    username TEXT NOT NULL,
    password_cleartext TEXT NOT NULL,
    nickname TEXT NOT NULL,
    source_info TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO pending_telegram_registrations_new (
    id,
    request_key,
    registrant_telegram_id,
    username,
    password_cleartext,
    nickname,
    source_info,
    created_at
)
SELECT
    id,
    request_key,
    registrant_telegram_id,
    username,
    password_cleartext,
    nickname,
    source_info,
    created_at
FROM pending_telegram_registrations;

DROP TABLE pending_telegram_registrations;
ALTER TABLE pending_telegram_registrations_new RENAME TO pending_telegram_registrations;

CREATE UNIQUE INDEX IF NOT EXISTS ix_pending_telegram_registrations_request_key
    ON pending_telegram_registrations(request_key);
CREATE INDEX IF NOT EXISTS ix_pending_telegram_registrations_created_at
    ON pending_telegram_registrations(created_at);
CREATE INDEX IF NOT EXISTS ix_pending_telegram_registrations_registrant_telegram_id
    ON pending_telegram_registrations(registrant_telegram_id);
CREATE INDEX IF NOT EXISTS ix_pending_telegram_registrations_username
    ON pending_telegram_registrations(username);
CREATE INDEX IF NOT EXISTS ix_pending_telegram_registrations_id
    ON pending_telegram_registrations(id);

CREATE TABLE pending_web_registrations_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    request_key TEXT NOT NULL UNIQUE,
    username TEXT NOT NULL,
    password_cleartext TEXT NOT NULL,
    nickname TEXT NOT NULL,
    ip_address TEXT NOT NULL,
    user_agent TEXT,
    source_info TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO pending_web_registrations_new (
    id,
    request_key,
    username,
    password_cleartext,
    nickname,
    ip_address,
    user_agent,
    source_info,
    created_at
)
SELECT
    id,
    request_key,
    username,
    password_cleartext,
    nickname,
    ip_address,
    user_agent,
    source_info,
    created_at
FROM pending_web_registrations;

DROP TABLE pending_web_registrations;
ALTER TABLE pending_web_registrations_new RENAME TO pending_web_registrations;

CREATE UNIQUE INDEX IF NOT EXISTS ix_pending_web_registrations_request_key
    ON pending_web_registrations(request_key);
CREATE INDEX IF NOT EXISTS ix_pending_web_registrations_username
    ON pending_web_registrations(username);
CREATE INDEX IF NOT EXISTS ix_pending_web_registrations_created_at
    ON pending_web_registrations(created_at);
CREATE INDEX IF NOT EXISTS ix_pending_web_registrations_id
    ON pending_web_registrations(id);
