CREATE TABLE session_dependency (
    session_id TEXT NOT NULL REFERENCES session (id),
    blocker_id TEXT NOT NULL REFERENCES session (id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    PRIMARY KEY (session_id, blocker_id)
) STRICT;
