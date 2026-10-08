

CREATE TABLE workspace (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    workspace_kind TEXT NOT NULL CHECK (workspace_kind IN ('local', 'remote')),
    workspace_id TEXT NOT NULL,
    client_id TEXT NOT NULL,
    server_origin TEXT,
    account_id TEXT,
    CHECK (
        (workspace_kind = 'local' AND server_origin IS NULL AND account_id IS NULL)
        OR
        (workspace_kind = 'remote' AND server_origin IS NOT NULL AND account_id IS NOT NULL)
    )
);

CREATE TABLE sync_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    dataset_id TEXT,
    canonical_seq INTEGER NOT NULL DEFAULT 0 CHECK (canonical_seq >= 0),
    authoritative_snapshot INTEGER NOT NULL DEFAULT 0 CHECK (authoritative_snapshot IN (0, 1))
);
INSERT INTO sync_state (singleton) VALUES (1);

CREATE TABLE canonical_resources (
    kind TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    value_version INTEGER NOT NULL,
    value_json BLOB NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    PRIMARY KEY (kind, resource_id)
);
CREATE INDEX canonical_resources_order
    ON canonical_resources(kind, ordinal, resource_id);

CREATE TABLE outbox (
    position INTEGER PRIMARY KEY AUTOINCREMENT,
    mutation_id TEXT NOT NULL UNIQUE,
    request_json BLOB NOT NULL,
    patch_json BLOB NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'awaiting_canonical', 'blocked')),
    sealed INTEGER NOT NULL DEFAULT 0 CHECK (sealed IN (0, 1)),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    next_attempt_at_ms INTEGER,
    last_error TEXT,
    receipt_json BLOB,
    receipt_commit_seq INTEGER,
    blocked_error_json BLOB,
    created_at_ms INTEGER NOT NULL
);
CREATE INDEX outbox_state_position ON outbox(state, position);

CREATE TABLE integration_entries (
    provider_id TEXT NOT NULL,
    external_id TEXT NOT NULL,
    external_version TEXT,
    item_type TEXT NOT NULL,
    internal_uuid TEXT NOT NULL,
    ignored INTEGER NOT NULL DEFAULT 0 CHECK (ignored IN (0, 1)),
    PRIMARY KEY (provider_id, external_id)
);
