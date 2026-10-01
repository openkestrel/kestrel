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
    -- The url names the base repository: a fork's pull request and one against the fork can share
    -- head repository and number.
    PRIMARY KEY (workspace_id, url)
) STRICT;

-- One per pull_request Event considered, whatever it matched, so each is considered once.
CREATE TABLE pull_request_attachment (
    event_record_id TEXT PRIMARY KEY REFERENCES event (record_id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    outcome TEXT NOT NULL CHECK (outcome IN ('attached', 'unmatched', 'ambiguous', 'sealed')),
    workspace_id TEXT REFERENCES workspace (id),
    considered_at TEXT NOT NULL,
    CHECK ((outcome = 'attached') = (workspace_id IS NOT NULL))
) STRICT;

-- Every Workspace an Event matched, with the state it was in at the time, so the Audit Record
-- `0.4` will explain an unmatched, ambiguous or sealed verdict from what was there then.
CREATE TABLE pull_request_candidate (
    event_record_id TEXT NOT NULL REFERENCES event (record_id),
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    state TEXT NOT NULL CHECK (state IN ('open', 'sealed')),
    PRIMARY KEY (event_record_id, workspace_id)
) STRICT;

-- One per distinct observation appended to a Workspace's Transcript: a delivery repeating one
-- already recorded changes nothing, however many Event rows a retry makes.
CREATE TABLE pull_request_observation (
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    url TEXT NOT NULL,
    action TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('open', 'closed', 'merged')),
    head_revision TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    event_record_id TEXT NOT NULL REFERENCES event (record_id),
    PRIMARY KEY (workspace_id, url, action, state, head_revision, updated_at)
) STRICT;
