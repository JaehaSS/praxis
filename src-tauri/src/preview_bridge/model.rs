use serde::{Deserialize, Serialize};

pub const MAX_RESULT_BYTES: usize = 512 * 1024;
pub const PREVIEW_PROBE_TASK_ID: i64 = -4_242;
pub const PREVIEW_PROBE_WEBVIEW_LABEL: &str = "designmode-probe";

pub fn new_session_id() -> Result<String, String> {
    random_hex_id()
}

pub fn new_command_id() -> Result<String, String> {
    random_hex_id()
}

pub fn random_hex_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRegistration {
    pub task_id: i64,
    pub webview_label: String,
    pub session_id: String,
    pub generation: u64,
}

impl SessionRegistration {
    pub fn new(
        task_id: i64,
        webview_label: impl Into<String>,
        session_id: impl Into<String>,
        generation: u64,
    ) -> Self {
        Self {
            task_id,
            webview_label: webview_label.into(),
            session_id: session_id.into(),
            generation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAction {
    pub task_id: i64,
    pub session_id: String,
    pub generation: u64,
    pub command_id: String,
}

impl PendingAction {
    pub fn new(
        task_id: i64,
        session_id: impl Into<String>,
        generation: u64,
        command_id: impl Into<String>,
    ) -> Self {
        Self {
            task_id,
            session_id: session_id.into(),
            generation,
            command_id: command_id.into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultEnvelope {
    pub task_id: i64,
    pub session_id: String,
    pub generation: u64,
    pub command_id: String,
    pub sha256: String,
    /// Final JSON payload text. Keeping this a string prevents Tauri IPC from
    /// serializing it as a numeric byte array.
    pub body: String,
}

impl ResultEnvelope {
    pub fn new(
        task_id: i64,
        session_id: impl Into<String>,
        generation: u64,
        command_id: impl Into<String>,
        sha256: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            task_id,
            session_id: session_id.into(),
            generation,
            command_id: command_id.into(),
            sha256: sha256.into(),
            body: body.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    PayloadTooLarge,
    InvalidJson,
    ShaMismatch,
    WebviewMismatch,
    TaskMismatch,
    SessionMismatch,
    GenerationMismatch,
    CommandMismatch,
    Duplicate,
    NoPending,
    Busy,
    TakenOver,
}

/// 대기자가 왜 끊겼는지. 세대가 올라 버려진 것(`Navigation`)과 사람이 회수한 것(`TakeOver`)은
/// 에이전트에게 다른 오류로 보여야 한다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    Navigation,
    TakeOver,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmitOutcome {
    Accepted { task_id: i64, command_id: String },
}
