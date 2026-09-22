//! 에이전트 명령을 실제 프리뷰 웹뷰로 내리는 Tauri 디스패처. MCP 서버는 이 파일을 모르고,
//! `Dispatcher` 트레이트 뒤에서만 만난다.

pub mod auto_open_probe;
pub mod events;
pub mod openings;

use std::time::Instant;

use serde_json::{json, Value};
use tauri::{Manager, Webview};
use tokio::sync::oneshot;

use crate::commands::{open_agent_preview, AppState};
use crate::db;
use crate::preview_bridge::mcp::dispatch::error_for_dropped_waiter;
use crate::preview_bridge::mcp::{deadline_for, is_loopback_origin, Command, DispatchError};
use crate::preview_bridge::{new_command_id, PendingAction};

pub(crate) use events::{control_event, is_still_idle, touch};
pub use events::{emit_control, emit_control_state, ControlEvent};

/// 명령이 끝난 뒤 이만큼 조용하면 제어 표시를 내린다.
const IDLE_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

/// 페이지가 우리 손을 떠난 뒤 스크립트가 도착하는 TOCTOU를 막는 마지막 관문 — 여기서 멈추면
/// 결과가 오지 않고 데드라인이 `timeout`으로 보고한다.
const ORIGIN_GUARD: &str =
    r#"if (!/^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/.test(location.origin)) return;"#;

/// 웹뷰에서 명령을 돌리고 결과를 IPC로 되돌리는 스크립트. 값은 JSON으로 이스케이프한다.
pub fn exec_script(
    task_id: i64,
    session_id: &str,
    generation: u64,
    command_id: &str,
    cmd: &Command,
) -> String {
    let session = Value::from(session_id).to_string();
    let command = Value::from(command_id).to_string();
    let cmd_json = cmd.to_exec_json();
    format!(
        "(function(){{{ORIGIN_GUARD}\
         var submit=function(body){{\
         return window.__praxisPreviewAgent.sha256Hex(body).then(function(sha){{\
         return window.__praxisPreviewAgent.submitResult({{\"taskId\":{task_id},\
         \"sessionId\":{session},\"generation\":{generation},\"commandId\":{command},\
         \"sha256\":sha,\"body\":body}});}});}};\
         window.__praxisPreviewExec.run({cmd_json}).then(submit).catch(function(e){{\
         return submit(JSON.stringify({{\"ok\":false,\"error\":\"bridge_failed\",\
         \"message\":String(e && e.message || e)}})).catch(function(){{}});}});}})();"
    )
}

/// 결과 본문에서 기록용 결과를 읽는다. 본문은 웹뷰가 만든 것이라 객체가 아닐 수 있다 —
/// 그때는 성공으로 본다(디스패치가 이미 성공을 돌려준 뒤다).
pub(crate) fn outcome_from_body(body: &str) -> (bool, Option<bool>, Option<String>) {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(body) else {
        return (true, None, None);
    };
    let ok = map.get("ok").and_then(Value::as_bool).unwrap_or(true);
    let changed = map.get("changed").and_then(Value::as_bool);
    let error = map
        .get("error")
        .or_else(|| map.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string);
    (ok, changed, error)
}

/// 본문에 실린 스냅샷의 크기. `console`처럼 스냅샷을 싣지 않는 op과 실패 응답에서는 None이다.
///
/// `bytes`·`nodes`가 없으면 통째로 포기한다 — 둘은 웹뷰가 항상 같이 싣고, 한쪽만 있는 본문은
/// 계측이 아니라 형식이 어긋난 응답이다. 0으로 채워 넣으면 집계가 그것을 공짜 스냅샷으로 센다.
pub(crate) fn snapshot_metrics_from_body(body: &str) -> Option<db::SnapshotMetrics> {
    let value = serde_json::from_str::<Value>(body).ok()?;
    let snapshot = value.get("snapshot")?.as_object()?;
    Some(db::SnapshotMetrics {
        bytes: snapshot.get("bytes").and_then(Value::as_i64)?,
        nodes: snapshot.get("nodes").and_then(Value::as_i64)?,
        truncated: snapshot
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        shrinks: snapshot.get("shrinks").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// 완료 이벤트가 보일 대상. 웹뷰가 사람이 읽는 이름(`button "로그인"`)을 실어 보내면 그것을 쓴다.
pub(crate) fn target_from_body(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("target")?
        .as_str()
        .map(str::to_string)
}

pub struct TauriDispatcher {
    pub app: tauri::AppHandle,
    pub pool: sqlx::SqlitePool,
}

#[async_trait::async_trait]
impl crate::preview_bridge::mcp::Dispatcher for TauriDispatcher {
    async fn dispatch(&self, task_id: i64, cmd: Command) -> Result<String, DispatchError> {
        let started = Instant::now();
        touch(&self.app, task_id);
        let outcome = self.run(task_id, &cmd).await;
        let elapsed = started.elapsed().as_millis() as i64;
        let (ok, changed, error) = match &outcome {
            Ok(body) => outcome_from_body(body),
            Err(error) => (false, None, Some(error.as_str().to_string())),
        };
        let snapshot = outcome
            .as_ref()
            .ok()
            .and_then(|body| snapshot_metrics_from_body(body));
        let _ = db::record_preview_command(
            &self.pool,
            db::PreviewCommandRecord {
                task_id,
                op: cmd.op(),
                ok,
                changed,
                elapsed_ms: elapsed,
                error: error.as_deref(),
                snapshot,
                now: crate::now(),
            },
        )
        .await;
        outcome
    }
}

impl TauriDispatcher {
    async fn run(&self, task_id: i64, cmd: &Command) -> Result<String, DispatchError> {
        let state = self.app.state::<AppState>();
        if let Command::Navigate { url } = cmd {
            let activation = open_agent_preview(&self.app, &state, task_id, url).await?;
            emit_control(&self.app, control_event(task_id, true, cmd, url, true));
            self.schedule_inactive(task_id, cmd, url, None);
            return Ok(json!({ "ok": true, "url": url, "opened": activation,
                "status": "navigation_started" })
            .to_string());
        }
        let webview = webview_of(&state, task_id).ok_or(DispatchError::NoPreview)?;
        let current = webview.url().map_err(|_| DispatchError::NoPreview)?;
        if !is_loopback_origin(&current) {
            return Err(DispatchError::NotControllableOrigin);
        }
        let current = current.to_string();
        let (rx, command_id) = self.start(&state, task_id, cmd, &webview)?;
        // 여기서부터 active를 냈으므로 어떤 결말이든 inactive를 예약해야 배지가 남지 않는다.
        emit_control(&self.app, control_event(task_id, true, cmd, &current, true));
        let outcome = wait_for_result(&state, task_id, cmd, rx, &command_id).await;
        self.schedule_inactive(task_id, cmd, &current, outcome.as_deref().ok());
        outcome
    }

    /// 대기자를 걸고 스크립트를 넣는다. 여기서 실패한 명령은 아직 제어 표시를 내지 않았다.
    fn start(
        &self,
        state: &AppState,
        task_id: i64,
        cmd: &Command,
        webview: &Webview,
    ) -> Result<(oneshot::Receiver<String>, String), DispatchError> {
        let (session_id, generation) = state
            .preview_bridge
            .session_of(task_id)
            .ok_or(DispatchError::NoPreview)?;
        let command_id = new_command_id().map_err(|_| DispatchError::NoPreview)?;
        let rx = state.preview_bridge.begin(PendingAction::new(
            task_id,
            &session_id,
            generation,
            &command_id,
        ))?;
        let script = exec_script(task_id, &session_id, generation, &command_id, cmd);
        webview
            .eval(script.as_str())
            .map_err(|_| DispatchError::NoPreview)?;
        Ok((rx, command_id))
    }

    /// 명령 뒤 3초 동안 새 명령이 없으면 제어 표시를 내린다. 마지막 액션이 무엇을 바꿨는지는
    /// 이 완료 이벤트에만 실린다 — 시작 이벤트 시점에는 아직 결과가 없다.
    fn schedule_inactive(&self, task_id: i64, cmd: &Command, url: &str, body: Option<&str>) {
        let mark = touch(&self.app, task_id);
        let app = self.app.clone();
        let event = control_event(task_id, false, cmd, url, true)
            .with_changed(body.and_then(|body| outcome_from_body(body).1))
            .with_target(body.and_then(target_from_body));
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(IDLE_AFTER).await;
            if !is_still_idle(&app, task_id, mark) {
                return;
            }
            emit_control(&app, event);
        });
    }
}

async fn wait_for_result(
    state: &AppState,
    task_id: i64,
    cmd: &Command,
    rx: oneshot::Receiver<String>,
    command_id: &str,
) -> Result<String, DispatchError> {
    match tokio::time::timeout(deadline_for(cmd), rx).await {
        Ok(Ok(body)) => Ok(body),
        // sender drop — 회수되었거나 네비게이션이 세대를 올렸다.
        Ok(Err(_)) => Err(error_for_dropped_waiter(
            state.preview_bridge.take_cancel_reason(task_id),
        )),
        Err(_) => {
            state.preview_bridge.cancel(task_id, command_id);
            Err(DispatchError::Timeout)
        }
    }
}

/// 락은 clone까지만 잡는다 — await를 건너 들고 가면 프리뷰 전체가 멈춘다.
pub(crate) fn webview_of(state: &AppState, task_id: i64) -> Option<Webview> {
    let webviews = state
        .designmode_webviews
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    webviews.get(&task_id).map(|handle| handle.webview.clone())
}

#[cfg(test)]
mod tests {
    use super::outcome_from_body;

    #[test]
    fn body_object_supplies_ok_and_changed() {
        assert_eq!(
            outcome_from_body(r#"{"ok":true,"changed":false,"text":""}"#),
            (true, Some(false), None)
        );
    }

    #[test]
    fn body_error_is_recorded_and_marks_failure() {
        assert_eq!(
            outcome_from_body(r#"{"ok":false,"error":"payload_too_large"}"#),
            (false, None, Some("payload_too_large".into()))
        );
    }

    #[test]
    fn non_json_body_counts_as_a_plain_success() {
        assert_eq!(outcome_from_body("not json"), (true, None, None));
    }

    #[test]
    fn snapshot_metrics_come_from_the_snapshot_object() {
        let body = r#"{"ok":true,"changed":true,"snapshot":
            {"text":"- button \"확인\"","nodes":12,"bytes":4096,"truncated":true,"shrinks":2}}"#;
        assert_eq!(
            super::snapshot_metrics_from_body(body),
            Some(crate::db::SnapshotMetrics {
                bytes: 4096,
                nodes: 12,
                truncated: true,
                shrinks: 2,
            })
        );
    }

    /// `console`은 `snapshot: false`를 주고 `resultFor`가 그것을 필드째 지운다. 실패 응답에도
    /// 스냅샷이 없다. 둘 다 0이 아니라 None이어야 집계가 공짜 스냅샷으로 세지 않는다.
    #[test]
    fn a_body_without_a_snapshot_has_no_metrics() {
        for body in [
            r#"{"ok":true,"entries":[]}"#,
            r#"{"ok":false,"error":"stale_ref"}"#,
            r#"{"ok":true,"snapshot":false}"#,
            "not json",
        ] {
            assert_eq!(super::snapshot_metrics_from_body(body), None, "{body}");
        }
    }

    /// 크기가 한쪽만 실려 오면 형식이 어긋난 응답이다 — 0으로 메우지 않고 통째로 버린다.
    #[test]
    fn a_half_filled_snapshot_is_not_measured() {
        let body = r#"{"ok":true,"snapshot":{"text":"x","bytes":100}}"#;
        assert_eq!(super::snapshot_metrics_from_body(body), None);
    }

    /// 옛 웹뷰가 남아 있어 `shrinks`·`truncated` 없이 와도 크기 둘은 살린다.
    #[test]
    fn missing_flags_default_instead_of_dropping_the_measurement() {
        let body = r#"{"ok":true,"snapshot":{"nodes":3,"bytes":90}}"#;
        assert_eq!(
            super::snapshot_metrics_from_body(body),
            Some(crate::db::SnapshotMetrics {
                bytes: 90,
                nodes: 3,
                truncated: false,
                shrinks: 0,
            })
        );
    }
}
