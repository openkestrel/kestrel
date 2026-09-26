-- A Workspace that predates sealing is open, and continues nothing.
ALTER TABLE workspace ADD COLUMN sealed_at TEXT;
ALTER TABLE workspace ADD COLUMN continues TEXT REFERENCES workspace (id);

CREATE INDEX workspace_continued_by ON workspace (continues) WHERE continues IS NOT NULL;

CREATE INDEX session_holding_a_slot ON session (workspace_id, state);
