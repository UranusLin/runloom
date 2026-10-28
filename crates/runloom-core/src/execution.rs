use crate::{
    store::{require_work, validate},
    Error, Store,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub directory: String,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub tty: bool,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub id: String,
    pub work_id: String,
    pub request_id: String,
    pub command: CommandSpec,
    pub state: String,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}
type Result<T> = std::result::Result<T, Error>;
impl Store {
    /// Internal supervisor lookup, not a scoped remote endpoint.
    pub fn execution_workspace(&self, id: &str) -> Result<String> {
        Ok(self.connection.query_row(
            "SELECT w.workspace FROM works w JOIN executions e ON e.work_id=w.id WHERE e.id=?1",
            [id],
            |row| row.get(0),
        )?)
    }

    /// Atomic admission. Retrying a request never launches a second worker.
    pub fn admit_execution(
        &mut self,
        workspace: &str,
        work: &str,
        request: &str,
        command: CommandSpec,
    ) -> Result<(Execution, bool)> {
        self.admit_guarded(workspace, work, request, command, None)
    }
    pub fn admit_guarded(
        &mut self,
        workspace: &str,
        work: &str,
        request: &str,
        command: CommandSpec,
        proof: Option<&crate::LeaseProof>,
    ) -> Result<(Execution, bool)> {
        if command.tty && !command.interactive {
            return Err(Error::Invalid("tty requires interactive control"));
        }
        if command.env.iter().any(|(k, v)| {
            !matches!(k.as_str(), "CODEX_HOME" | "CLAUDE_CONFIG_DIR") || v.contains('\0')
        }) {
            return Err(Error::Invalid("profile environment"));
        }
        validate(request, "request_id")?;
        validate(&command.executable, "executable")?;
        validate(&command.directory, "directory")?;
        if !std::path::Path::new(&command.executable).is_absolute()
            || !std::path::Path::new(&command.directory).is_absolute()
            || command.args.iter().any(|a| a.contains('\0'))
        {
            return Err(Error::Invalid("command"));
        }
        let encoded = serde_json::to_string(&command)?;
        if encoded.len() > 32768 {
            return Err(Error::Invalid("command size"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_work(&tx, workspace, work)?;
        if command.interactive || proof.is_some() {
            crate::control::check(&tx, work, proof.ok_or(Error::StaleLease)?)?;
        }
        let previous: Option<String> = tx
            .query_row(
                "SELECT id FROM executions WHERE work_id=?1 AND request_id=?2",
                params![work, request],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = previous {
            tx.commit()?;
            let prior = self.execution(&id)?;
            if prior.command != command {
                return Err(Error::RequestConflict);
            }
            return Ok((prior, false));
        }
        let identity=tx.query_row("SELECT workspace,host,provider,config_root,native_id,directory,title FROM works WHERE id=?1",[work],|r| Ok(crate::NewWork { workspace:r.get(0)?,host:r.get(1)?,provider:r.get(2)?,config_root:r.get(3)?,native_id:r.get(4)?,directory:r.get(5)?,title:r.get(6)? }))?;
        if identity.directory != command.directory
            && !matches!(
                (std::fs::canonicalize(&identity.directory), std::fs::canonicalize(&command.directory)),
                (Ok(registered), Ok(requested)) if registered == requested
            )
        {
            return Err(Error::Invalid(
                "execution directory differs from registered work",
            ));
        }
        crate::reservation::check_reserved(&tx, &identity, &command.directory)?;
        let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM executions WHERE work_id=?1 AND state IN ('queued','running'))",[work],|r|r.get(0))?;
        if active {
            return Err(Error::ActiveExecution);
        }
        // Workspace labels must not permit two writers to the same native session.
        let (host, provider, config, native): (String, String, String, String) = tx.query_row(
            "SELECT host,provider,config_root,native_id FROM works WHERE id=?1",
            [work],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let mut query=tx.prepare("SELECT w.config_root FROM works w JOIN executions e ON e.work_id=w.id WHERE w.host=?1 AND w.provider=?2 AND w.native_id=?3 AND e.state IN ('queued','running')")?;
        for other in query.query_map(params![host, provider, native], |r| r.get::<_, String>(0))? {
            let other = other?;
            if other == config
                || match (
                    std::fs::canonicalize(&other),
                    std::fs::canonicalize(&config),
                ) {
                    (Ok(a), Ok(b)) => a == b,
                    _ => true,
                }
            {
                return Err(Error::ActiveExecution);
            }
        }
        drop(query);
        let id = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO executions(id,work_id,request_id,command_json,state) VALUES (?1,?2,?3,?4,'queued')",params![id,work,request,encoded])?;
        tx.commit()?;
        Ok((self.execution(&id)?, true))
    }
    /// Trusted worker/admin lookup; callers must scope untrusted requests first.
    pub fn latest_execution(&self, workspace: &str, work: &str) -> Result<Option<Execution>> {
        require_work(&self.connection, workspace, work)?;
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM executions WHERE work_id=?1 ORDER BY rowid DESC LIMIT 1",
                [work],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| self.execution(&id)).transpose()
    }
    pub fn execution(&self, id: &str) -> Result<Execution> {
        let row=self.connection.query_row("SELECT id,work_id,request_id,command_json,state,exit_code,error FROM executions WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,Option<i32>>(5)?,r.get::<_,Option<String>>(6)?))).optional()?.ok_or(Error::NotFound)?;
        Ok(Execution {
            id: row.0,
            work_id: row.1,
            request_id: row.2,
            command: serde_json::from_str(&row.3)?,
            state: row.4,
            exit_code: row.5,
            error: row.6,
        })
    }
    pub fn scoped_execution(&self, workspace: &str, work: &str, id: &str) -> Result<Execution> {
        require_work(&self.connection, workspace, work)?;
        let execution = self.execution(id)?;
        if execution.work_id != work {
            return Err(Error::NotFound);
        }
        Ok(execution)
    }
    pub fn claim_execution(&mut self, id: &str) -> Result<()> {
        if self.connection.execute(
            "UPDATE executions SET state='running' WHERE id=?1 AND state='queued'",
            [id],
        )? != 1
        {
            return Err(Error::StateConflict);
        }
        Ok(())
    }
    pub fn finish_execution(
        &mut self,
        id: &str,
        exit_code: Option<i32>,
        error: Option<&str>,
    ) -> Result<()> {
        let state = if error.is_some() { "failed" } else { "exited" };
        if self.connection.execute("UPDATE executions SET state=?1,exit_code=?2,error=?3 WHERE id=?4 AND state IN ('queued','running')",params![state,exit_code,error,id])?!=1 {return Err(Error::StateConflict);}
        Ok(())
    }
}
