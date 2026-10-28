CREATE TABLE workspaces (id TEXT PRIMARY KEY NOT NULL);
CREATE TABLE works (
    id TEXT PRIMARY KEY NOT NULL,
    workspace TEXT NOT NULL REFERENCES workspaces(id),
    host TEXT NOT NULL,
    provider TEXT NOT NULL,
    config_root TEXT NOT NULL,
    native_id TEXT NOT NULL,
    directory TEXT NOT NULL,
    title TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'registered' CHECK (state = 'registered'),
    control_epoch INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(workspace, host, provider, config_root, native_id)
);
CREATE TABLE events (
    work_id TEXT NOT NULL REFERENCES works(id),
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    request_id TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(work_id, sequence),
    UNIQUE(work_id, request_id)
);
PRAGMA user_version = 1;
