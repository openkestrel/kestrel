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

-- History, never authority: it outlives its Instance and decides no hold, seal or release.
CREATE TABLE instance_work_report (
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    instance TEXT NOT NULL,
    repositories TEXT NOT NULL,
    reported_at TEXT NOT NULL,
    PRIMARY KEY (workspace_id, instance)
) STRICT;
