CREATE TABLE pull_request (
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    repository TEXT NOT NULL,
    number INTEGER NOT NULL CHECK (number > 0),
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('open', 'closed', 'merged')),
    head_branch TEXT NOT NULL,
    head_revision TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    event_record_id TEXT NOT NULL REFERENCES event (record_id),
    PRIMARY KEY (workspace_id, repository, number)
) STRICT;

-- One per pull_request Event considered, whatever it matched, so each is considered once.
CREATE TABLE pull_request_attachment (
    event_record_id TEXT PRIMARY KEY REFERENCES event (record_id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    outcome TEXT NOT NULL CHECK (outcome IN ('attached', 'unmatched', 'ambiguous')),
    workspace_id TEXT REFERENCES workspace (id),
    considered_at TEXT NOT NULL,
    CHECK ((outcome = 'attached') = (workspace_id IS NOT NULL))
) STRICT;
