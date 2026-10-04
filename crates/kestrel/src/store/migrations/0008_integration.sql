CREATE TABLE integration (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL REFERENCES organization (id),
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('github', 'webhook')),
    repository TEXT,
    api TEXT,
    app_id INTEGER,
    installation_id INTEGER,
    private_key_sealed TEXT,
    bot_login TEXT,
    inbound INTEGER NOT NULL,
    outbound INTEGER NOT NULL,
    interval_ms INTEGER,
    signing_secret TEXT,
    signed INTEGER NOT NULL DEFAULT 0,
    shared_secret_digest TEXT,
    poll_due_at TEXT,
    polled_through INTEGER,
    last_event_refusal_source TEXT,
    last_event_refusal_id TEXT,
    last_event_refusal_bytes INTEGER,
    last_event_refusal_reason TEXT,
    last_event_refusal_at TEXT,
    registered_at TEXT NOT NULL,
    UNIQUE (organization_id, name),
    CHECK ((kind = 'github') = (repository IS NOT NULL AND api IS NOT NULL
                                AND app_id IS NOT NULL AND installation_id IS NOT NULL
                                AND private_key_sealed IS NOT NULL AND bot_login IS NOT NULL
                                AND interval_ms IS NOT NULL)),
    CHECK (kind = 'github' OR signing_secret IS NULL),
    CHECK ((kind = 'webhook') = (shared_secret_digest IS NOT NULL))
) STRICT;

CREATE INDEX integration_poll_due ON integration (poll_due_at) WHERE poll_due_at IS NOT NULL;

CREATE TABLE event (
    record_id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL REFERENCES organization (id),
    -- NULL for an Event kestrel minted itself.
    integration_id TEXT REFERENCES integration (id),
    id TEXT NOT NULL CHECK (id <> ''),
    source TEXT NOT NULL CHECK (source <> ''),
    specversion TEXT NOT NULL CHECK (specversion = '1.0'),
    type TEXT NOT NULL CHECK (type <> ''),
    subject TEXT,
    time TEXT NOT NULL,
    data TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    UNIQUE (organization_id, source, id)
) STRICT;

CREATE INDEX event_by_organization ON event (organization_id, time);

CREATE TABLE github_app_flow (
    state TEXT PRIMARY KEY,
    phase TEXT NOT NULL CHECK (phase IN ('ready', 'exchanging', 'converted')),
    expires_at TEXT NOT NULL,
    configuration_sealed TEXT NOT NULL
) STRICT;
