-- An explicit test of saved material: one model call in a short-lived Instance. Kept so a Client
-- that lost its connection reads the test it started instead of starting another.
CREATE TABLE sign_in_test (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL REFERENCES organization (id),
    harness TEXT NOT NULL,
    method TEXT NOT NULL,
    profile_id TEXT REFERENCES subscription_profile (id),
    revision INTEGER NOT NULL REFERENCES material_revision (revision),
    requested_model TEXT,
    model TEXT,
    image TEXT,
    state TEXT NOT NULL CHECK (state IN (
        'pending', 'running', 'completed', 'failed', 'interrupted'
    )),
    outcome TEXT CHECK (outcome IN (
        'worked', 'authentication_failed', 'not_covered', 'unavailable', 'harness_failed',
        'material_replaced'
    )),
    message TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT
) STRICT;
