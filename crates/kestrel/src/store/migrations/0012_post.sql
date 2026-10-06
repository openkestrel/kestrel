-- One thing kestrel says back where the work came from: a completed Turn's response, and a Session's
-- own final Outcome when it adds something the Turn responses did not. `turn` is the Turn's seq,
-- or 0 for the Session's own, so a message is recorded once and a retry recognises its own comment.
-- `attempted_at` is set before the request goes out and left set, so a control plane that died
-- mid-post finds a row that says a comment may already be there and reads it back rather than
-- posting a second one.
CREATE TABLE post (
    session_id TEXT NOT NULL REFERENCES session (id),
    turn INTEGER NOT NULL,
    organization_id TEXT NOT NULL REFERENCES organization (id),
    integration_id TEXT NOT NULL REFERENCES integration (id),
    event_record_id TEXT NOT NULL REFERENCES event (record_id),
    subject INTEGER NOT NULL,
    body TEXT NOT NULL,
    turn_messages TEXT,
    attempted_at TEXT,
    due_at TEXT,
    posted_at TEXT,
    posted_to TEXT,
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (session_id, turn)
) STRICT;

CREATE INDEX post_due ON post (due_at) WHERE due_at IS NOT NULL;
