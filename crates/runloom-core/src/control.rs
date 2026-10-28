use crate::{
    store::{require_work, validate},
    Error, Store,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseProof {
    pub epoch: i64,
    pub token: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseStatus {
    pub holder: String,
    pub epoch: i64,
    pub expires_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseGrant {
    pub status: LeaseStatus,
    pub proof: LeaseProof,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputRequest {
    pub workspace: String,
    pub work: String,
    pub execution: String,
    pub request_id: String,
    pub proof: LeaseProof,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResizeRequest {
    pub workspace: String,
    pub work: String,
    pub execution: String,
    pub proof: LeaseProof,
    pub cols: u16,
    pub rows: u16,
}
fn now() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Invalid("clock"))?
        .as_millis()
        .try_into()
        .map_err(|_| Error::Invalid("clock"))
}
fn expiry(ttl: u32) -> Result<i64> {
    if !(1..=300).contains(&ttl) {
        return Err(Error::Invalid("lease ttl"));
    }
    Ok(now()? + i64::from(ttl) * 1000)
}
pub(crate) fn check(db: &Connection, work: &str, proof: &LeaseProof) -> Result<()> {
    let valid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM leases WHERE work_id=?1 AND epoch=?2 AND token=?3 AND expires_ms>?4)",params![work,proof.epoch,proof.token,now()?],|r|r.get(0))?;
    if valid {
        Ok(())
    } else {
        Err(Error::StaleLease)
    }
}
impl Store {
    pub fn resize_terminal(
        &mut self,
        request: &ResizeRequest,
        resize: impl FnOnce() -> std::io::Result<()>,
    ) -> Result<()> {
        if !(1..=500).contains(&request.cols) || !(1..=300).contains(&request.rows) {
            return Err(Error::Invalid("terminal dimensions"));
        }
        let execution =
            self.scoped_execution(&request.workspace, &request.work, &request.execution)?;
        if !execution.command.tty || execution.state != "running" {
            return Err(Error::Invalid("terminal not running"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check(&tx, &request.work, &request.proof)?;
        resize().map_err(|_| Error::Invalid("terminal resize failed"))?;
        tx.commit()?;
        Ok(())
    }

    pub fn input_status(
        &self,
        workspace: &str,
        work: &str,
        execution: &str,
        request_id: &str,
    ) -> Result<Option<String>> {
        self.scoped_execution(workspace, work, execution)?;
        Ok(self
            .connection
            .query_row(
                "SELECT state FROM input_receipts WHERE execution_id=?1 AND request_id=?2",
                params![execution, request_id],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn lease_status(&self, workspace: &str, work: &str) -> Result<Option<LeaseStatus>> {
        require_work(&self.connection, workspace, work)?;
        Ok(self
            .connection
            .query_row(
                "SELECT holder,epoch,expires_ms FROM leases WHERE work_id=?1",
                [work],
                |r| {
                    Ok(LeaseStatus {
                        holder: r.get(0)?,
                        epoch: r.get(1)?,
                        expires_ms: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }
    pub fn acquire_lease(
        &mut self,
        workspace: &str,
        work: &str,
        holder: &str,
        ttl: u32,
        takeover_epoch: Option<i64>,
    ) -> Result<LeaseGrant> {
        validate(holder, "holder")?;
        let expires = expiry(ttl)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_work(&tx, workspace, work)?;
        let epoch: i64 =
            tx.query_row("SELECT control_epoch FROM works WHERE id=?1", [work], |r| {
                r.get(0)
            })?;
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM leases WHERE work_id=?1 AND expires_ms>?2)",
            params![work, now()?],
            |r| r.get(0),
        )?;
        if takeover_epoch.is_some_and(|e| e != epoch) || (active && takeover_epoch != Some(epoch)) {
            return Err(Error::LeaseConflict);
        }
        let epoch = epoch
            .checked_add(1)
            .ok_or(Error::Invalid("epoch overflow"))?;
        let token = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "UPDATE works SET control_epoch=?1 WHERE id=?2",
            params![epoch, work],
        )?;
        tx.execute("INSERT INTO leases(work_id,holder,epoch,token,expires_ms) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(work_id) DO UPDATE SET holder=excluded.holder,epoch=excluded.epoch,token=excluded.token,expires_ms=excluded.expires_ms",params![work,holder,epoch,token,expires])?;
        tx.commit()?;
        Ok(LeaseGrant {
            status: LeaseStatus {
                holder: holder.into(),
                epoch,
                expires_ms: expires,
            },
            proof: LeaseProof { epoch, token },
        })
    }
    pub fn renew_lease(
        &mut self,
        workspace: &str,
        work: &str,
        proof: &LeaseProof,
        ttl: u32,
    ) -> Result<()> {
        let expires = expiry(ttl)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_work(&tx, workspace, work)?;
        check(&tx, work, proof)?;
        tx.execute(
            "UPDATE leases SET expires_ms=?1 WHERE work_id=?2",
            params![expires, work],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn release_lease(&mut self, workspace: &str, work: &str, proof: &LeaseProof) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_work(&tx, workspace, work)?;
        check(&tx, work, proof)?;
        tx.execute("UPDATE leases SET expires_ms=0 WHERE work_id=?1", [work])?;
        tx.commit()?;
        Ok(())
    }
    /// Mark intent durably BEFORE writing. Any crash leaves uncertainty, never a resend.
    pub fn prepare_input(&mut self, input: &InputRequest) -> Result<bool> {
        validate(&input.request_id, "request_id")?;
        if input.bytes.is_empty() || input.bytes.len() > 4096 {
            return Err(Error::Invalid("input size"));
        }
        let execution = self.scoped_execution(&input.workspace, &input.work, &input.execution)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check(&tx, &input.work, &input.proof)?;
        let prior: Option<(Vec<u8>, String)> = tx
            .query_row(
                "SELECT payload,state FROM input_receipts WHERE execution_id=?1 AND request_id=?2",
                params![input.execution, input.request_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((bytes, state)) = prior {
            if bytes != input.bytes {
                return Err(Error::RequestConflict);
            }
            return if state == "delivered" {
                Ok(false)
            } else {
                Err(Error::UncertainInput)
            };
        }
        if execution.state != "running" || !execution.command.interactive {
            return Err(Error::StateConflict);
        }
        tx.execute("INSERT INTO input_receipts(execution_id,request_id,payload,state) VALUES (?1,?2,?3,'uncertain')",params![input.execution,input.request_id,input.bytes])?;
        tx.commit()?;
        Ok(true)
    }
    /// The bounded write holds the same SQLite writer lock as takeover: linearizable fencing.
    pub fn deliver_input(
        &mut self,
        input: &InputRequest,
        write: impl FnOnce(&[u8]) -> std::io::Result<()>,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_work(&tx, &input.workspace, &input.work)?;
        if check(&tx, &input.work, &input.proof).is_err() {
            tx.execute("UPDATE input_receipts SET state='rejected' WHERE execution_id=?1 AND request_id=?2 AND state='uncertain'",params![input.execution,input.request_id])?;
            tx.commit()?;
            return Err(Error::StaleLease);
        }
        let bytes:Vec<u8>=tx.query_row("SELECT payload FROM input_receipts WHERE execution_id=?1 AND request_id=?2 AND state='uncertain'",params![input.execution,input.request_id],|r|r.get(0))?;
        if bytes != input.bytes {
            return Err(Error::RequestConflict);
        }
        write(&bytes).map_err(|_| Error::UncertainInput)?;
        tx.execute(
            "UPDATE input_receipts SET state='delivered' WHERE execution_id=?1 AND request_id=?2",
            params![input.execution, input.request_id],
        )?;
        tx.commit()?;
        Ok(())
    }
}
