ALTER TABLE session RENAME COLUMN started_at TO enqueued_at;

-- Every Session inserted from here on names its own state; the default is what a Session that
-- predates kestrel scheduling one gets, and such a Session is over.
ALTER TABLE session ADD COLUMN state TEXT NOT NULL DEFAULT 'ended';
ALTER TABLE session ADD COLUMN claimed_at TEXT;
ALTER TABLE session ADD COLUMN started_at TEXT;
ALTER TABLE session ADD COLUMN heartbeat_at TEXT;
ALTER TABLE session ADD COLUMN instance TEXT;
ALTER TABLE session ADD COLUMN exit TEXT;
ALTER TABLE session ADD COLUMN exit_because TEXT;
ALTER TABLE session ADD COLUMN outcome_message TEXT;

CREATE TABLE turn (
    session_id TEXT NOT NULL REFERENCES session (id),
    organization_id TEXT NOT NULL REFERENCES organization (id),
    seq INTEGER NOT NULL,
    prompted_at TEXT NOT NULL,
    -- The last Transcript entry before the prompt, so this Turn's response is what follows it.
    from_seq INTEGER NOT NULL DEFAULT 0,
    answered_at TEXT,
    PRIMARY KEY (session_id, seq)
) STRICT;
