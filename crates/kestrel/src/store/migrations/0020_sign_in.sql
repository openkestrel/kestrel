CREATE TABLE authentication_evidence (
    revision INTEGER PRIMARY KEY REFERENCES material_revision (revision),
    state TEXT NOT NULL CHECK (state IN (
        'unchecked', 'login_completed', 'credential_accepted', 'authentication_failed', 'expired',
        'not_covered'
    )),
    source TEXT NOT NULL CHECK (source IN (
        'import', 'provider_check', 'relay', 'generic_write', 'refresh', 'session'
    )),
    observed_at TEXT NOT NULL,
    provider_check TEXT
) STRICT;

CREATE TABLE model_use_evidence (
    revision INTEGER NOT NULL REFERENCES material_revision (revision),
    harness TEXT NOT NULL,
    model TEXT NOT NULL,
    image TEXT NOT NULL,
    result TEXT NOT NULL CHECK (result IN ('worked', 'authentication_failed', 'not_covered')),
    source TEXT NOT NULL CHECK (source IN ('model_test', 'session')),
    observed_at TEXT NOT NULL,
    PRIMARY KEY (revision, harness, model)
) STRICT;
