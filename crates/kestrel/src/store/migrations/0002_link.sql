CREATE TABLE session (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    organization_id TEXT NOT NULL REFERENCES organization (id),
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    agent_id TEXT NOT NULL REFERENCES agent (id),
    harness TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    connected_at TEXT,
    supervisor_version TEXT,
    UNIQUE (organization_id, name)
) STRICT;

CREATE TABLE session_credential (
    token_hash TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session (id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    issued_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    invalidated_at TEXT
) STRICT;

CREATE TABLE link_instruction (
    session_id TEXT NOT NULL REFERENCES session (id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    seq INTEGER NOT NULL,
    body TEXT NOT NULL,
    sent_at TEXT NOT NULL,
    PRIMARY KEY (session_id, seq)
) STRICT;
