//! Read-only recovery evidence. Absence of rows never proves a process is dead.
use crate::{store::require_work, Error, Store};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ReservationEvidence {
    pub id: String,
    pub directory: String,
    pub created_ms: i64,
    pub guards_native_identity: bool,
}
#[derive(Debug, Serialize)]
pub struct ExecutionEvidence {
    pub execution_id: String,
    pub persisted_state: String,
}
#[derive(Debug, Serialize)]
pub struct InputEvidence {
    pub execution_id: String,
    pub request_id: String,
}
#[derive(Debug, Serialize)]
pub struct RecoveryInspection {
    pub workspace: String,
    pub work: String,
    pub reservations: Vec<ReservationEvidence>,
    pub unsettled_executions: Vec<ExecutionEvidence>,
    pub uncertain_inputs: Vec<InputEvidence>,
    pub truncated: bool,
    pub process_liveness_checked: bool,
    pub automatic_recovery_available: bool,
}

impl Store {
    /// A consistent scoped database snapshot, without tokens, commands or input
    /// payloads. Includes at most 100 rows in each category. Does not inspect or
    /// modify filesystem state, terminate workers, release fences or replay input.
    pub fn recovery_inspect(
        &mut self,
        workspace: &str,
        work: &str,
    ) -> Result<RecoveryInspection, Error> {
        let tx = self.connection.transaction()?;
        require_work(&tx, workspace, work)?;
        let mut reservations = tx.prepare("SELECT id,directory,created_ms,guard_native FROM checkpoint_reservations WHERE work_id=?1 ORDER BY created_ms,id,directory LIMIT 101")?
            .query_map([work], |r| Ok(ReservationEvidence { id:r.get(0)?,directory:r.get(1)?,created_ms:r.get(2)?,guards_native_identity:r.get(3)? }))?
            .collect::<Result<Vec<_>,_>>()?;
        let mut executions = tx.prepare("SELECT id,state FROM executions WHERE work_id=?1 AND state IN ('queued','running') ORDER BY id LIMIT 101")?
            .query_map([work], |r| Ok(ExecutionEvidence { execution_id:r.get(0)?,persisted_state:r.get(1)? }))?
            .collect::<Result<Vec<_>,_>>()?;
        let mut inputs = tx.prepare("SELECT i.execution_id,i.request_id FROM input_receipts i JOIN executions e ON e.id=i.execution_id WHERE e.work_id=?1 AND i.state='uncertain' ORDER BY i.execution_id,i.request_id LIMIT 101")?
            .query_map([work], |r| Ok(InputEvidence { execution_id:r.get(0)?,request_id:r.get(1)? }))?
            .collect::<Result<Vec<_>,_>>()?;
        let truncated = reservations.len() > 100 || executions.len() > 100 || inputs.len() > 100;
        reservations.truncate(100);
        executions.truncate(100);
        inputs.truncate(100);
        tx.commit()?;
        Ok(RecoveryInspection {
            workspace: workspace.into(),
            work: work.into(),
            reservations,
            unsettled_executions: executions,
            uncertain_inputs: inputs,
            truncated,
            process_liveness_checked: false,
            automatic_recovery_available: false,
        })
    }
}
