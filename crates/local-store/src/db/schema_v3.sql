CREATE TABLE local_exports (
    position INTEGER PRIMARY KEY AUTOINCREMENT,
    export_id TEXT NOT NULL UNIQUE,
    server_origin TEXT NOT NULL,
    account_id TEXT NOT NULL,
    batches_json BLOB NOT NULL
);

CREATE TABLE local_imports (
    position INTEGER PRIMARY KEY AUTOINCREMENT,
    export_id TEXT NOT NULL UNIQUE,
    batches_json BLOB
);
