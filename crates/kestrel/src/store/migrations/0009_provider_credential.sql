-- Minted for every write of a Provider Credential or Profile entry and never reused, so a write
-- after a delete still reads as new material.
CREATE TABLE material_revision (
    revision INTEGER PRIMARY KEY AUTOINCREMENT,
    written_at TEXT NOT NULL,
    -- The revision a harness's own refresh advanced, so it is not read as a replacement.
    refreshed_from INTEGER REFERENCES material_revision (revision)
) STRICT;

CREATE TABLE provider_credential (
    organization_id TEXT NOT NULL REFERENCES organization (id),
    variable TEXT NOT NULL,
    sealed TEXT NOT NULL,
    set_at TEXT NOT NULL,
    revision INTEGER NOT NULL REFERENCES material_revision (revision),
    PRIMARY KEY (organization_id, variable)
) STRICT;
