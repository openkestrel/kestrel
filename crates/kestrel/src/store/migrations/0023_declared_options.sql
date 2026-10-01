-- The three categories an Agent, Trigger or Session may declare, each a Harness value id; a
-- category named none for is the Harness's own default (ADR-0041). The Session's are the ones
-- resolved at enqueue: its own over the Trigger's over the Agent's.
ALTER TABLE agent ADD COLUMN mode TEXT;
ALTER TABLE agent ADD COLUMN thought_level TEXT;

ALTER TABLE trigger ADD COLUMN model TEXT;
ALTER TABLE trigger ADD COLUMN mode TEXT;
ALTER TABLE trigger ADD COLUMN thought_level TEXT;

ALTER TABLE session ADD COLUMN mode TEXT;
ALTER TABLE session ADD COLUMN thought_level TEXT;

ALTER TABLE pending_session ADD COLUMN model TEXT;
ALTER TABLE pending_session ADD COLUMN mode TEXT;
ALTER TABLE pending_session ADD COLUMN thought_level TEXT;
