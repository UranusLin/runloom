//! Persistent filesystem-operation fencing, independent of short SQLite locks.
use crate::{Error, NewWork};
use rusqlite::Connection;
use std::path::Path;

fn same_native(a: &NewWork, provider: &str, config: &str, native: &str) -> bool {
    a.provider == provider
        && a.native_id == native
        && (a.config_root == config
            || match (
                std::fs::canonicalize(&a.config_root),
                std::fs::canonicalize(config),
            ) {
                (Ok(a), Ok(b)) => a == b,
                _ => true,
            })
}

/// Called inside the same immediate transaction as an admission or reservation.
pub(crate) fn check_reserved(
    db: &Connection,
    work: &NewWork,
    directory: &str,
) -> Result<(), Error> {
    // Unavailable paths fail closed when any reservation needs comparison.
    let path = std::fs::canonicalize(directory).ok();
    check_reserved_inner(db, work, path.as_deref())
}

pub(crate) fn check_reserved_path(
    db: &Connection,
    work: &NewWork,
    path: &Path,
) -> Result<(), Error> {
    check_reserved_inner(db, work, Some(path))
}

fn check_reserved_inner(db: &Connection, work: &NewWork, path: Option<&Path>) -> Result<(), Error> {
    let mut q=db.prepare("SELECT r.id,r.directory,w.provider,w.config_root,w.native_id,r.guard_native FROM checkpoint_reservations r JOIN works w ON w.id=r.work_id WHERE w.host=?1")?;
    let rows = q.query_map([&work.host], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, bool>(5)?,
        ))
    })?;
    for row in rows {
        let (id, reserved, provider, config, native, guard_native) = row?;
        let collision = match path {
            Some(path) => path.starts_with(&reserved) || Path::new(&reserved).starts_with(path),
            None => true,
        };
        if collision || (guard_native && same_native(work, &provider, &config, &native)) {
            return Err(Error::WorkspaceReserved(id));
        }
    }
    Ok(())
}

pub(crate) fn check_active(db: &Connection, work: &NewWork, source: &Path) -> Result<(), Error> {
    check_active_inner(db, work, source, true)
}

fn check_active_inner(
    db: &Connection,
    work: &NewWork,
    source: &Path,
    guard_native: bool,
) -> Result<(), Error> {
    let mut q=db.prepare("SELECT e.command_json,w.provider,w.config_root,w.native_id FROM works w JOIN executions e ON e.work_id=w.id WHERE w.host=?1 AND e.state IN ('queued','running')")?;
    let rows = q.query_map([&work.host], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (command, provider, config, native) = row?;
        // Inspect the actual execution directory, including records admitted by
        // older core versions before directory identity was enforced here.
        let command: crate::CommandSpec = serde_json::from_str(&command)?;
        let active =
            std::fs::canonicalize(command.directory).map_err(|_| Error::ActiveExecution)?;
        if active.starts_with(source)
            || source.starts_with(active)
            || (guard_native && same_native(work, &provider, &config, &native))
        {
            return Err(Error::ActiveExecution);
        }
    }
    Ok(())
}

impl crate::Store {
    /// Trusted worker API for a durably admitted branch job. This does not admit
    /// new client work or replace lease validation at the remote intake boundary.
    /// It fences only the new destination so the immutable checkpoint's original
    /// session may continue while the branch is being materialized.
    pub fn with_reserved_destination<T>(
        &mut self,
        workspace: &str,
        work: &str,
        destination: &Path,
        operation: impl FnOnce() -> Result<T, Box<dyn std::error::Error + Send + Sync>>,
    ) -> Result<T, Error> {
        if !destination.is_absolute()
            || destination
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(Error::Invalid("branch destination"));
        }
        let target = std::fs::canonicalize(
            destination
                .parent()
                .ok_or(Error::Invalid("destination parent"))?,
        )
        .map_err(|_| Error::Invalid("destination parent"))?
        .join(
            destination
                .file_name()
                .ok_or(Error::Invalid("destination name"))?,
        );
        let selected = self
            .list(workspace)?
            .into_iter()
            .find(|w| w.id == work)
            .ok_or(Error::NotFound)?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        check_active_inner(&tx, &selected.identity, &target, false)?;
        check_reserved_path(&tx, &selected.identity, &target)?;
        let reservation = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO checkpoint_reservations(id,work_id,directory,created_ms,guard_native) VALUES (?1,?2,?3,CAST(strftime('%s','now') AS INTEGER)*1000,0)",rusqlite::params![reservation,work,target.to_str().ok_or(Error::Invalid("destination encoding"))?])?;
        tx.commit()?;
        let result = operation().map_err(|e| Error::Checkpoint(e.to_string()));
        self.connection.execute(
            "DELETE FROM checkpoint_reservations WHERE id=?1",
            [&reservation],
        )?;
        result
    }
}
