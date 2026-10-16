CREATE TABLE sign_in_relay (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL REFERENCES organization (id),
    profile_id TEXT NOT NULL REFERENCES subscription_profile (id),
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    harness TEXT NOT NULL,
    method TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'starting', 'awaiting_browser', 'awaiting_code', 'saving',
        'completed', 'failed', 'expired', 'cancelled'
    )),
    claimed INTEGER NOT NULL DEFAULT 0,
    cleaned INTEGER NOT NULL DEFAULT 0,
    baseline INTEGER REFERENCES material_revision (revision),
    version INTEGER NOT NULL,
    deadline INTEGER NOT NULL,
    finished_at INTEGER,
    snapshot TEXT
) STRICT;

CREATE UNIQUE INDEX one_active_sign_in_relay_per_destination
ON sign_in_relay (profile_id, kind, name)
WHERE state IN ('starting', 'awaiting_browser', 'awaiting_code', 'saving');
