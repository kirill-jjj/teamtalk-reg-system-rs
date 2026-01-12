CREATE INDEX IF NOT EXISTS idx_telegram_registrations_teamtalk_username
    ON telegram_registrations(teamtalk_username);

CREATE INDEX IF NOT EXISTS idx_pending_registrations_request_key
    ON pending_telegram_registrations(request_key);

CREATE INDEX IF NOT EXISTS idx_pending_registrations_created_at
    ON pending_telegram_registrations(created_at);

CREATE INDEX IF NOT EXISTS idx_banned_users_banned_at
    ON banned_users(banned_at DESC);

CREATE INDEX IF NOT EXISTS idx_download_tokens_expires_used
    ON fastapi_download_tokens(expires_at, is_used);

CREATE INDEX IF NOT EXISTS idx_registered_ips_timestamp
    ON fastapi_registered_ips(registration_timestamp);

CREATE INDEX IF NOT EXISTS idx_deeplink_tokens_expires_used
    ON deeplink_tokens(expires_at, is_used);
