//! Advisory planning only. A plan is never authority to mutate or replay work.
use crate::{Error, RecoveryInspection, Store};
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDisposition {
    EvidenceIncomplete,
    ManualInvestigationRequired,
    NoRecordedBlockers,
}

#[derive(Debug, Serialize)]
pub struct RecoveryStep {
    pub code: &'static str,
    pub instruction: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RecoveryPlan {
    pub plan_version: u32,
    pub disposition: RecoveryDisposition,
    pub evidence: RecoveryInspection,
    pub steps: Vec<RecoveryStep>,
    pub apply_supported: bool,
    pub prohibited_actions: Vec<&'static str>,
    pub limitations: Vec<&'static str>,
}

impl RecoveryPlan {
    fn from_inspection(evidence: RecoveryInspection) -> Self {
        let mut steps = Vec::new();
        let mut add = |code, instruction| steps.push(RecoveryStep { code, instruction });
        if evidence.truncated {
            add("inspect_complete_evidence", "The response is capped. Preserve the node root and obtain a complete administrator-reviewed inventory before drawing conclusions from these rows.");
        }
        if !evidence.reservations.is_empty() {
            add("preserve_checkpoint_artifacts", "Keep reservation fences and partial source/destination artifacts intact. Establish the original operation owner and verify it has stopped; age, an expired lease or a node restart is not proof.");
        }
        if !evidence.unsettled_executions.is_empty() {
            add("verify_execution_owner", "Inspect the selected execution's status, worker and provider history. Queued/running rows are persisted evidence only. A dead supervisor does not prove its child or external effects have stopped.");
        }
        if !evidence.uncertain_inputs.is_empty() {
            add("reconcile_uncertain_input", "Match each execution/request ID against output and provider history. The input may already have taken effect. Keep the receipt and do not resend it under the same or a new request ID.");
        }
        let recorded = !evidence.reservations.is_empty()
            || !evidence.unsettled_executions.is_empty()
            || !evidence.uncertain_inputs.is_empty();
        if recorded || evidence.truncated {
            add("review_resolution_separately", "After collecting ownership and effect evidence, review a resolution separately. This release cannot apply a repair. Do not manually delete fences just to retry an operation.");
        } else {
            add("inspect_other_operation_state", "No blockers were recorded in these three categories. Inspect branch/task receipts, overlapping work and unmanaged processes separately; this is not a safe-to-resume verdict.");
        }
        Self {
            plan_version: 1,
            disposition: if evidence.truncated {
                RecoveryDisposition::EvidenceIncomplete
            } else if recorded {
                RecoveryDisposition::ManualInvestigationRequired
            } else {
                RecoveryDisposition::NoRecordedBlockers
            },
            evidence,
            steps,
            apply_supported: false,
            prohibited_actions: vec![
                "automatic_input_replay",
                "automatic_task_restart",
                "reservation_deletion",
                "infer_process_death_from_age",
            ],
            limitations: vec![
                "point_in_time_work_scoped_database_snapshot",
                "process_liveness_and_owner_generation_not_verified",
                "branch_task_files_and_external_effects_not_inspected",
                "overlapping_other_work_not_enumerated",
                "not_an_apply_token_or_safety_authorization",
            ],
        }
    }
}

impl Store {
    /// Build instructions from one consistent inspection. No database or file repair.
    pub fn recovery_plan(&mut self, workspace: &str, work: &str) -> Result<RecoveryPlan, Error> {
        Ok(RecoveryPlan::from_inspection(
            self.recovery_inspect(workspace, work)?,
        ))
    }
}
