use crate::Result;
use runloom_core::{CommandSpec, InputRequest, LeaseProof, NewWork, ResizeRequest};
use serde::Deserialize;
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};
#[derive(Deserialize)]
pub struct Request {
    pub version: u32,
    #[serde(flatten)]
    pub operation: Operation,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Ping,
    RecoveryPlan {
        workspace: String,
        work: String,
    },
    RecoveryInspect {
        workspace: String,
        work: String,
    },
    TaskCreate {
        spec: crate::task::TaskSpec,
        #[serde(default)]
        dangerous_ack: bool,
    },
    TaskInfo {
        workspace: String,
        work: String,
    },
    TaskStart {
        workspace: String,
        work: String,
        proof: LeaseProof,
    },
    CheckpointFork {
        request: crate::branch::BranchRequest,
        proof: LeaseProof,
    },
    BranchList {
        workspace: String,
        work: String,
    },
    BranchStatus {
        workspace: String,
        work: String,
        request_id: String,
    },
    LatestExecution {
        workspace: String,
        work: String,
    },
    Resize {
        resize: ResizeRequest,
    },
    InputStatus {
        workspace: String,
        work: String,
        execution: String,
        request_id: String,
    },
    CheckpointList {
        workspace: String,
        work: String,
    },
    CheckpointCapture {
        workspace: String,
        work: String,
        proof: LeaseProof,
        #[serde(default)]
        untracked: Vec<String>,
        #[serde(default)]
        native_session: Option<String>,
        #[serde(default)]
        auto_native: bool,
        #[serde(default)]
        parent: Option<String>,
    },
    CheckpointRestore {
        workspace: String,
        work: String,
        proof: LeaseProof,
        checkpoint: String,
        destination: String,
    },
    CheckpointInspect {
        workspace: String,
        work: String,
        checkpoint: String,
    },
    Resume {
        workspace: String,
        work: String,
        proof: LeaseProof,
        request_id: String,
        executable: String,
        #[serde(default)]
        prompt: Option<String>,
        headless: bool,
    },
    ResumePlan {
        workspace: String,
        work: String,
        executable: String,
        #[serde(default)]
        prompt: Option<String>,
        headless: bool,
    },
    LeaseStatus {
        workspace: String,
        work: String,
    },
    Acquire {
        workspace: String,
        work: String,
        holder: String,
        ttl: u32,
        #[serde(default)]
        takeover_epoch: Option<i64>,
    },
    Renew {
        workspace: String,
        work: String,
        proof: LeaseProof,
        ttl: u32,
    },
    Release {
        workspace: String,
        work: String,
        proof: LeaseProof,
    },
    Input {
        input: InputRequest,
    },
    Register {
        work: NewWork,
    },
    List {
        workspace: String,
    },
    Start {
        workspace: String,
        work: String,
        request_id: String,
        command: CommandSpec,
        #[serde(default)]
        proof: Option<LeaseProof>,
    },
    Status {
        workspace: String,
        work: String,
        execution: String,
    },
    Events {
        workspace: String,
        work: String,
        after: i64,
        limit: u32,
    },
}
pub fn read_frame(reader: impl Read, max: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    BufReader::new(reader.take(max + 1)).read_until(b'\n', &mut bytes)?;
    if bytes.len() as u64 > max || bytes.last() != Some(&b'\n') {
        return Err("invalid or oversized newline-delimited frame".into());
    }
    Ok(bytes)
}
pub fn bridge(root: &Path) -> Result<()> {
    let input = read_frame(std::io::stdin().lock(), 65536)?;
    let mut stream = UnixStream::connect(root.join("node.sock"))?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(&input)?;
    let response = read_frame(stream, 8 * 1024 * 1024)?;
    std::io::stdout().write_all(&response)?;
    let value: serde_json::Value = serde_json::from_slice(&response)?;
    if value["ok"] != true {
        return Err("request rejected (see JSON response)".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_and_operation_validation() {
        assert!(read_frame(&b"{}"[..], 100).is_err());
        assert!(read_frame(&b"123456\n"[..], 3).is_err());
        assert!(serde_json::from_str::<Request>(r#"{"version":1,"op":"ping"}"#).is_ok());
        assert!(
            serde_json::from_str::<Request>(r#"{"version":1,"op":"start","surprise":true}"#)
                .is_err()
        );
    }
}
