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

-- Starting another supervisor on an Instance replaces its row, and letting the Instance go deletes it.
CREATE TABLE supervisor (
    instance TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL REFERENCES organization (id),
    token_hash TEXT NOT NULL UNIQUE,
    name TEXT,
    version TEXT,
    started_at TEXT NOT NULL,
    reached_at TEXT
) STRICT;

CREATE TABLE link_instruction (
    instance TEXT NOT NULL,
    seq INTEGER NOT NULL,
    session_id TEXT NOT NULL REFERENCES session (id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    body TEXT NOT NULL,
    sent_at TEXT NOT NULL,
    PRIMARY KEY (instance, seq)
) STRICT;
