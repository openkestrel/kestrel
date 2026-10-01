-- An option change a person asked for on a live Session: held from the write until the harness
-- answers it, and cleared with it (ADR-0041).
ALTER TABLE session ADD COLUMN changing_options TEXT;
