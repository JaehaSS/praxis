use crate::db::{self, Task};
use crate::preview_bridge::mcp::is_loopback_origin;

use super::AppState;

pub(super) fn require_main(webview: &tauri::Webview) -> Result<(), String> {
    if is_main_webview(webview.label(), webview.window().label()) {
        return Ok(());
    }
    Err("Preview Workbench는 메인 창에서만 사용할 수 있습니다".into())
}

fn is_main_webview(webview_label: &str, window_label: &str) -> bool {
    webview_label == "main" && window_label == "main"
}

pub(super) async fn task_of(state: &AppState, task_id: i64) -> Result<Task, String> {
    db::get_task(&super::pool_of(state)?, task_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "작업을 찾을 수 없습니다".into())
}

pub(super) fn current_url(state: &AppState, task_id: i64) -> Result<Option<String>, String> {
    let Some(webview) = crate::preview_control::webview_of(state, task_id) else {
        return Ok(None);
    };
    webview
        .url()
        .map(|url| Some(url.to_string()))
        .map_err(|error| error.to_string())
}

pub(super) fn ensure_current_url(state: &AppState, task_id: i64, url: &str) -> Result<(), String> {
    let Some(actual) = current_url(state, task_id)? else {
        return Err("preview_url_changed".into());
    };
    let parsed = tauri::Url::parse(&actual).map_err(|error| error.to_string())?;
    if actual == url && is_loopback_origin(&parsed) {
        return Ok(());
    }
    Err("preview_url_changed".into())
}

pub(super) fn ensure_supported(task: &Task) -> Result<(), String> {
    if task.mode != "conversation" {
        return Err("대화 모드 작업만 지원합니다".into());
    }
    if matches!(task.agent.as_deref(), Some("claude") | Some("codex")) {
        return Ok(());
    }
    Err("Claude 또는 Codex 로컬 대화 작업만 지원합니다".into())
}

pub(super) fn unsupported_reason(task: &Task) -> Option<String> {
    ensure_supported(task).err()
}

#[cfg(test)]
mod tests {
    use super::is_main_webview;

    #[test]
    fn main_root_webview_is_allowed_but_inline_preview_is_not() {
        assert!(is_main_webview("main", "main"));
        assert!(!is_main_webview("designmode-7-1", "main"));
    }
}
