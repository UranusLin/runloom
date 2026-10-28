use crate::{Event, NewWork, Work};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{path::Path, time::Duration};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("work not found in the selected workspace")]
    NotFound,
    #[error("this provider session is already registered in the workspace")]
    DuplicateSession,
    #[error("request ID already used for a different event")]
    RequestConflict,
    #[error("an execution is already active or its outcome is unknown")]
    ActiveExecution,
    #[error("workspace reserved by checkpoint {0}; inspect unfinished operation before recovery")]
    WorkspaceReserved(String),
    #[error("execution state transition rejected")]
    StateConflict,
    #[error("control lease is held or takeover epoch changed")]
    LeaseConflict,
    #[error("expired or stale control lease")]
    StaleLease,
    #[error("input outcome uncertain; automatic resend prohibited")]
    UncertainInput,
    #[error("checkpoint operation failed: {0}")]
    Checkpoint(String),
    #[error("database schema {0} is newer than this binary supports")]
    FutureSchema(i64),
}
impl Error {
    pub fn is_busy(&self) -> bool {
        matches!(self,Self::Database(rusqlite::Error::SqliteFailure(error,_)) if matches!(error.code,rusqlite::ErrorCode::DatabaseBusy|rusqlite::ErrorCode::DatabaseLocked))
    }
}
type Result<T> = std::result::Result<T, Error>;

pub struct Store {
    pub(crate) connection: Connection,
}

pub(crate) fn validate(value: &str, field: &'static str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        Err(Error::Invalid(field))
    } else {
        Ok(())
    }
}

impl Store {
    pub fn with_idle_work<T>(
        &mut self,
        workspace: &str,
        work: &str,
        proof: &crate::LeaseProof,
        operation: impl FnOnce(Work) -> std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>,
    ) -> Result<T> {
        self.with_idle_work_paths(workspace, work, proof, &[], operation)
    }

    /// Reserve source and destination paths atomically without holding a writer
    /// transaction during filesystem I/O. Destination parents must exist.
    pub fn with_idle_work_paths<T>(
        &mut self,
        workspace: &str,
        work: &str,
        proof: &crate::LeaseProof,
        destinations: &[&Path],
        operation: impl FnOnce(Work) -> std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>,
    ) -> Result<T> {
        let selected = self
            .list(workspace)?
            .into_iter()
            .find(|w| w.id == work)
            .ok_or(Error::NotFound)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        crate::control::check(&tx, work, proof)?;
        let source = std::fs::canonicalize(&selected.identity.directory)
            .map_err(|_| Error::Invalid("checkpoint directory"))?;
        let mut directories = vec![source];
        for path in destinations {
            if !path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(Error::Invalid("checkpoint destination"));
            }
            let parent =
                std::fs::canonicalize(path.parent().ok_or(Error::Invalid("destination parent"))?)
                    .map_err(|_| Error::Invalid("destination parent"))?;
            directories
                .push(parent.join(path.file_name().ok_or(Error::Invalid("destination name"))?));
        }
        directories.sort();
        directories.dedup();
        for directory in &directories {
            crate::reservation::check_active(&tx, &selected.identity, directory)?;
            crate::reservation::check_reserved_path(&tx, &selected.identity, directory)?;
        }
        let reservation = uuid::Uuid::new_v4().to_string();
        for directory in directories {
            tx.execute("INSERT INTO checkpoint_reservations(id,work_id,directory,created_ms) VALUES (?1,?2,?3,CAST(strftime('%s','now') AS INTEGER)*1000)",params![reservation,work,directory.to_str().ok_or(Error::Invalid("checkpoint path encoding"))?])?;
        }
        tx.commit()?;
        // Long Git/filesystem I/O must never hold the global database writer lock.
        // A panic or process death deliberately leaves this durable reservation.
        let result = operation(selected).map_err(|e| Error::Checkpoint(e.to_string()));
        self.connection.execute(
            "DELETE FROM checkpoint_reservations WHERE id=?1",
            [&reservation],
        )?;
        let result = result?;
        Ok(result)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        // Migrations are serialized across processes, including first startup.
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 => {
                tx.execute_batch(include_str!("schema.sql"))?;
                tx.execute_batch(include_str!("schema-v2.sql"))?;
            }
            1 => tx.execute_batch(include_str!("schema-v2.sql"))?,
            2 => (),
            3 | 4 => (),
            n => return Err(Error::FutureSchema(n)),
        }
        if version < 3 {
            tx.execute_batch(include_str!("schema-v3.sql"))?;
        }
        if version < 4 {
            tx.execute_batch(include_str!("schema-v4.sql"))?;
        }
        tx.commit()?;
        Ok(Self { connection })
    }

    pub fn create_work(&mut self, input: NewWork) -> Result<Work> {
        for (value, field) in [
            (&input.workspace, "workspace"),
            (&input.host, "host"),
            (&input.provider, "provider"),
            (&input.config_root, "config_root"),
            (&input.native_id, "native_id"),
            (&input.directory, "directory"),
            (&input.title, "title"),
        ] {
            validate(value, field)?;
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM works WHERE workspace=?1 AND host=?2 AND provider=?3 AND config_root=?4 AND native_id=?5)",
            params![input.workspace,input.host,input.provider,input.config_root,input.native_id], |r| r.get(0))?;
        if exists {
            return Err(Error::DuplicateSession);
        }
        tx.execute(
            "INSERT OR IGNORE INTO workspaces(id) VALUES (?1)",
            [&input.workspace],
        )?;
        let id = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO works(id,workspace,host,provider,config_root,native_id,directory,title) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id,input.workspace,input.host,input.provider,input.config_root,input.native_id,input.directory,input.title])?;
        let work = Work {
            id,
            identity: input,
            state: "registered".into(),
        };
        tx.execute(
            "INSERT INTO events(work_id,sequence,kind,payload) VALUES (?1,1,'work.registered',?2)",
            params![work.id, serde_json::to_string(&work)?],
        )?;
        tx.commit()?;
        Ok(work)
    }

    pub fn list(&self, workspace: &str) -> Result<Vec<Work>> {
        validate(workspace, "workspace")?;
        let mut query = self.connection.prepare("SELECT id,workspace,host,provider,config_root,native_id,directory,title,state FROM works WHERE workspace=?1 ORDER BY created_at,id")?;
        let rows = query.query_map([workspace], |r| {
            Ok(Work {
                id: r.get(0)?,
                identity: NewWork {
                    workspace: r.get(1)?,
                    host: r.get(2)?,
                    provider: r.get(3)?,
                    config_root: r.get(4)?,
                    native_id: r.get(5)?,
                    directory: r.get(6)?,
                    title: r.get(7)?,
                },
                state: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Request deduplication here applies to event recording, not agent execution.
    pub fn append_event(
        &mut self,
        workspace: &str,
        work_id: &str,
        request_id: &str,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<Event> {
        validate(request_id, "request_id")?;
        validate(kind, "kind")?;
        if kind.starts_with("work.") {
            return Err(Error::Invalid("reserved event kind"));
        }
        let json = serde_json::to_string(&payload)?;
        if json.len() > 65536 {
            return Err(Error::Invalid("payload size"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_work(&tx, workspace, work_id)?;
        let prior: Option<(i64, String, String)> = tx
            .query_row(
                "SELECT sequence,kind,payload FROM events WHERE work_id=?1 AND request_id=?2",
                params![work_id, request_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((sequence, old_kind, old_json)) = prior {
            if old_kind != kind || old_json != json {
                return Err(Error::RequestConflict);
            }
            return Ok(Event {
                sequence,
                kind: old_kind,
                payload,
                request_id: Some(request_id.into()),
            });
        }
        let sequence: i64 = tx.query_row(
            "SELECT COALESCE(MAX(sequence),0)+1 FROM events WHERE work_id=?1",
            [work_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO events(work_id,sequence,kind,payload,request_id) VALUES (?1,?2,?3,?4,?5)",
            params![work_id, sequence, kind, json, request_id],
        )?;
        tx.commit()?;
        Ok(Event {
            sequence,
            kind: kind.into(),
            payload,
            request_id: Some(request_id.into()),
        })
    }

    pub fn events(
        &self,
        workspace: &str,
        work_id: &str,
        after: i64,
        limit: u32,
    ) -> Result<Vec<Event>> {
        if after < 0 || !(1..=1000).contains(&limit) {
            return Err(Error::Invalid("cursor/limit"));
        }
        require_work(&self.connection, workspace, work_id)?;
        let mut query=self.connection.prepare("SELECT sequence,kind,payload,request_id FROM events WHERE work_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3")?;
        let rows = query.query_map(params![work_id, after, limit], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (sequence, kind, json, request_id) = row?;
            Ok(Event {
                sequence,
                kind,
                payload: serde_json::from_str(&json)?,
                request_id,
            })
        })
        .collect()
    }
}

pub(crate) fn require_work(db: &Connection, workspace: &str, id: &str) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM works WHERE workspace=?1 AND id=?2)",
        params![workspace, id],
        |r| r.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(Error::NotFound)
    }
}
