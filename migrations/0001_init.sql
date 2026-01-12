CREATE TABLE IF NOT EXISTS telegram_registrations (
    telegram_id INTEGER PRIMARY KEY,
    teamtalk_username TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS pending_telegram_registrations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_key TEXT UNIQUE,
    registrant_telegram_id INTEGER,
    username TEXT,
    password_cleartext TEXT,
    nickname TEXT,
    source_info TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS banned_users (
    telegram_id INTEGER PRIMARY KEY,
    teamtalk_username TEXT,
    banned_at DATETIME NOT NULL,
    banned_by_admin_id INTEGER,
    reason TEXT
);

CREATE TABLE IF NOT EXISTS fastapi_download_tokens (
    token TEXT PRIMARY KEY,
    filepath_on_server TEXT NOT NULL,
    original_filename TEXT NOT NULL,
    token_type TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    expires_at DATETIME NOT NULL,
    is_used BOOLEAN DEFAULT 0
);

CREATE TABLE IF NOT EXISTS fastapi_registered_ips (
    ip_address TEXT PRIMARY KEY,
    username TEXT,
    registration_timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS deeplink_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token TEXT UNIQUE,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME NOT NULL,
    is_used BOOLEAN DEFAULT 0,
    generated_by_admin_id INTEGER
);
