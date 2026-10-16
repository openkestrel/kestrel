CREATE TABLE webhook_secret_rotation (
    integration_id TEXT PRIMARY KEY REFERENCES integration (id),
    replacement_sealed TEXT NOT NULL,
    started_at TEXT NOT NULL,
    attempted_at TEXT,
    failure TEXT
        CHECK (failure IN ('credentials_rejected', 'refused', 'rate_limited', 'unanswered')),
    retry_at TEXT,
    CHECK (failure IS NULL OR attempted_at IS NOT NULL),
    CHECK ((failure IS 'rate_limited') = (retry_at IS NOT NULL))
);
