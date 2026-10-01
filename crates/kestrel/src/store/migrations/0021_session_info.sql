-- What the Harness told the supervisor about the Session beyond its Turn reports: its own title
-- for the conversation, its whole config-option list, and the commands it offers (ADR-0041).
ALTER TABLE session ADD COLUMN title TEXT;
ALTER TABLE session ADD COLUMN config_options TEXT;
ALTER TABLE session ADD COLUMN commands TEXT;
