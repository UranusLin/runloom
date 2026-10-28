use serde::{Deserialize, Serialize};

/// Provider is an extensible adapter identifier, not a closed Claude/Codex enum.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewWork {
    pub workspace: String,
    pub host: String,
    pub provider: String,
    pub config_root: String,
    pub native_id: String,
    pub directory: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Work {
    pub id: String,
    #[serde(flatten)]
    pub identity: NewWork,
    /// Metadata registration is not evidence of a running execution.
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub sequence: i64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub request_id: Option<String>,
}
