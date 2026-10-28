-- Durable fencing for long checkpoint I/O. No expiry: a crashed operation must
-- be inspected, never guessed idle merely because its original lease expired.
CREATE TABLE checkpoint_reservations (
 id TEXT NOT NULL,
 work_id TEXT NOT NULL REFERENCES works(id),
 directory TEXT NOT NULL,
 created_ms INTEGER NOT NULL,
 guard_native INTEGER NOT NULL DEFAULT 1 CHECK(guard_native IN (0,1)),
 PRIMARY KEY(id, directory)
);
CREATE INDEX checkpoint_reservations_work ON checkpoint_reservations(work_id);
PRAGMA user_version = 4;
