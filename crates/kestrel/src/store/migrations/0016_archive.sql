-- An Instance leaves its Workspace in the transaction that seals or releases it, and waits here for
-- a work role to destroy it, so a work role that is down when a Workspace seals still finds it.
CREATE TABLE instance_archive (
    instance TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL REFERENCES organization (id),
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    queued_at TEXT NOT NULL
) STRICT;

CREATE TABLE instance_idle_hint (
    organization_id TEXT NOT NULL REFERENCES organization (id),
    workspace_id TEXT PRIMARY KEY REFERENCES workspace (id),
    instance TEXT NOT NULL,
    idle_since TEXT NOT NULL,
    archive_deadline TEXT NOT NULL
) STRICT;

-- One row per Instance, kept after the Workspace lets it go, so a replacement never inherits it.
-- Information only: the numbered checkout report in workspace.observed alone gates reaping.
CREATE TABLE instance_work_report (
    instance TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL REFERENCES organization (id),
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    repositories TEXT NOT NULL,
    reported_at TEXT NOT NULL
) STRICT;

CREATE INDEX instance_work_report_workspace ON instance_work_report (workspace_id);
