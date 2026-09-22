use sha2::{Digest, Sha256};

use super::model::{CancelReason, RejectReason, ResultEnvelope};
use super::registry::SessionState;

pub(super) fn clear_for_navigation(state: &mut SessionState, generation: u64, expected: bool) {
    state.registration.generation = generation;
    state.pending = None;
    if state.waiter.take().is_some() {
        state.last_cancel = Some(CancelReason::Navigation);
    }
    state.fallback_command = None;
    state.completed_command = None;
    state.expected_navigation = expected;
}

pub(super) fn is_command_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn validate_preview_probe_url(value: &str) -> Result<tauri::Url, String> {
    let url = tauri::Url::parse(value).map_err(|error| error.to_string())?;
    let host = url
        .host_str()
        .ok_or("probe URL must include a loopback host")?;
    if url.scheme() != "http" || !matches!(host, "localhost" | "127.0.0.1") {
        return Err("probe URL must use loopback http".to_string());
    }
    if url.port().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("probe URL requires a port and no userinfo".to_string());
    }
    Ok(url)
}

pub(super) fn validate_identity(
    state: &SessionState,
    caller: &str,
    result: &ResultEnvelope,
) -> Result<(), RejectReason> {
    if state.registration.webview_label != caller {
        return Err(RejectReason::WebviewMismatch);
    }
    if state.registration.session_id != result.session_id {
        return Err(RejectReason::SessionMismatch);
    }
    if state.registration.generation != result.generation {
        return Err(RejectReason::GenerationMismatch);
    }
    Ok(())
}

pub(super) fn no_pending_reason(state: &SessionState, result: &ResultEnvelope) -> RejectReason {
    if state.completed_command.as_deref() == Some(result.command_id.as_str()) {
        RejectReason::Duplicate
    } else {
        RejectReason::NoPending
    }
}
