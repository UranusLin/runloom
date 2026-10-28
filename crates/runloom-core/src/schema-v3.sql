CREATE TABLE leases (
 work_id TEXT PRIMARY KEY NOT NULL REFERENCES works(id),
 holder TEXT NOT NULL,
 epoch INTEGER NOT NULL,
 token TEXT NOT NULL,
 expires_ms INTEGER NOT NULL
);
CREATE TABLE input_receipts (
 execution_id TEXT NOT NULL REFERENCES executions(id),
 request_id TEXT NOT NULL,
 payload BLOB NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('uncertain','delivered','rejected')),
 PRIMARY KEY(execution_id,request_id)
);
PRAGMA user_version = 3;
