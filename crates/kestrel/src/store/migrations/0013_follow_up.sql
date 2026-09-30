ALTER TABLE integration ADD COLUMN comments_polled_through INTEGER;

ALTER TABLE event ADD COLUMN message TEXT;

ALTER TABLE session ADD COLUMN supervisor TEXT;

CREATE TABLE pending_message (
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    seq INTEGER NOT NULL,
    participant TEXT NOT NULL,
    body TEXT NOT NULL,
    received_at TEXT NOT NULL,
    PRIMARY KEY (workspace_id, seq)
) STRICT;

-- A firing's new Session, waiting for the Workspace's unfinished one to let go.
CREATE TABLE pending_session (
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    seq INTEGER NOT NULL,
    agent_id TEXT NOT NULL REFERENCES agent (id),
    trigger TEXT NOT NULL,
    brief TEXT NOT NULL,
    received_at TEXT NOT NULL,
    PRIMARY KEY (workspace_id, seq)
) STRICT;

CREATE TABLE follow_up (
    event_record_id TEXT PRIMARY KEY REFERENCES event (record_id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    received_at TEXT NOT NULL
) STRICT;
