-- A Session holds a lease from the moment it is claimed until it ends, and what a sweep needs is
-- the due time rather than the moment its Environment was last heard from.
ALTER TABLE session DROP COLUMN heartbeat_at;
ALTER TABLE session ADD COLUMN lease_expires_at TEXT;

CREATE INDEX session_lease_due ON session (lease_expires_at) WHERE lease_expires_at IS NOT NULL;
