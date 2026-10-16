CREATE TABLE hook_url_update (
    integration_id TEXT PRIMARY KEY REFERENCES integration(id),
    url TEXT NOT NULL
);
