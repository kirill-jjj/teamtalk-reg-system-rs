-- Tighten schema after compatibility migration.
-- Drops invalid pending rows and enforces NOT NULL constraints.

CREATE TABLE pending_telegram_registrations_new (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    request_key TEXT NOT NULL UNIQUE,
    registrant_telegram_id INTEGER NOT NULL,
    username TEXT NOT NULL,
    password_cleartext TEXT NOT NULL,
    nickname TEXT NOT NULL,
    source_info TEXT NOT NULL,
    created_at DATETIME NOT NULL
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
    COALESCE(request_key, 'legacy_' || id),
    COALESCE(registrant_telegram_id, 0),
    COALESCE(username, 'legacy_user_' || id),
    COALESCE(password_cleartext, ''),
    COALESCE(nickname, COALESCE(username, 'legacy_user_' || id)),
    COALESCE(source_info, 'legacy'),
    COALESCE(created_at, CURRENT_TIMESTAMP)
FROM pending_telegram_registrations
;

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
    created_at DATETIME NOT NULL
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
    COALESCE(request_key, 'legacy_' || id),
    COALESCE(username, 'legacy_user_' || id),
    COALESCE(password_cleartext, ''),
    COALESCE(nickname, COALESCE(username, 'legacy_user_' || id)),
    COALESCE(ip_address, '0.0.0.0'),
    user_agent,
    COALESCE(source_info, 'legacy'),
    COALESCE(created_at, CURRENT_TIMESTAMP)
FROM pending_web_registrations
;

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
