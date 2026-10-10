ALTER TABLE session ADD COLUMN sign_in_method TEXT;

-- What a Session's harness was last spawned with, kept after the material is replaced or
-- forgotten so the Session's diagnosis still names the revision it used.
CREATE TABLE session_material (
    session_id TEXT NOT NULL REFERENCES session (id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    profile_id TEXT REFERENCES subscription_profile (id),
    kind TEXT NOT NULL CHECK (kind IN ('variable', 'file')),
    name TEXT NOT NULL,
    revision INTEGER NOT NULL REFERENCES material_revision (revision),
    handed_at TEXT NOT NULL,
    PRIMARY KEY (session_id, kind, name),
    CHECK (kind = 'variable' OR profile_id IS NOT NULL)
) STRICT;

-- A Profile's login in use by something that is not a Session, which a Session on the same
-- serialized harness waits behind.
CREATE TABLE credential_use (
    profile_id TEXT NOT NULL REFERENCES subscription_profile (id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    harness TEXT NOT NULL,
    holder TEXT NOT NULL,
    acquired_at TEXT NOT NULL,
    PRIMARY KEY (profile_id, harness)
) STRICT;
