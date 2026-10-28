CREATE TABLE executions (
 id TEXT PRIMARY KEY NOT NULL,
 work_id TEXT NOT NULL REFERENCES works(id),
 request_id TEXT NOT NULL,
 command_json TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('queued','running','exited','failed')),
 exit_code INTEGER,
 error TEXT,
 UNIQUE(work_id,request_id)
);
CREATE UNIQUE INDEX one_active_execution ON executions(work_id) WHERE state IN ('queued','running');
PRAGMA user_version = 2;
