//! 제어 표시 이벤트 하나 — 에이전트 명령과 사용자 회수·해제가 같은 채널을 쓴다.
//! 유휴 표시(`touch`·`is_still_idle`)도 여기 산다. 언제 이벤트를 낼지를 정하는 것이 전부다.

use std::collections::HashMap;
use std::sync::MutexGuard;
use std::time::Instant;

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::commands::AppState;
use crate::preview_bridge::mcp::Command;

/// 제어 상태를 프론트에 알리는 이벤트. 키는 `DesignCapturePayload` 관행대로 스네이크.
#[derive(Clone, Serialize)]
pub struct ControlEvent {
    pub task_id: i64,
    pub active: bool,
    pub op: String,
    pub target: Option<String>,
    pub changed: Option<bool>,
    pub url: String,
    pub controllable: bool,
}

impl ControlEvent {
    /// `changed`는 명령이 끝나야 알 수 있다 — 시작 이벤트는 실을 것이 없고, 완료 이벤트만 싣는다.
    pub(crate) fn with_changed(mut self, changed: Option<bool>) -> Self {
        self.changed = changed;
        self
    }

    /// 본문이 말하는 대상이 ref를 이긴다 — 웹뷰만 `s1e3`이 `button "로그인"`인 줄 안다.
    /// 본문에 없으면 시작 이벤트와 같은 ref·URL을 그대로 둔다.
    pub(crate) fn with_target(mut self, target: Option<String>) -> Self {
        if target.is_some() {
            self.target = target;
        }
        self
    }
}

pub fn emit_control(app: &tauri::AppHandle, event: ControlEvent) {
    let _ = app.emit("designmode://control", event);
}

/// 사용자 조작으로 제어권이 오간 것을 알린다 — 명령이 아니므로 언제나 `active:false`다.
pub fn emit_control_state(
    app: &tauri::AppHandle,
    state: &AppState,
    task_id: i64,
    op: &str,
    controllable: bool,
) {
    let url = super::webview_of(state, task_id)
        .and_then(|webview| webview.url().ok())
        .map(|url| url.to_string())
        .unwrap_or_default();
    emit_control(
        app,
        ControlEvent {
            task_id,
            active: false,
            op: op.to_string(),
            target: None,
            changed: None,
            url,
            controllable,
        },
    );
}

/// 네비게이션의 `target`·`url`은 둘 다 인자로 받은 목적지다 — 떠나기 전 주소를 실으면
/// 프론트가 한 박자 늦은 곳을 가리킨다.
pub(crate) fn control_event(
    task_id: i64,
    active: bool,
    cmd: &Command,
    url: &str,
    controllable: bool,
) -> ControlEvent {
    ControlEvent {
        task_id,
        active,
        op: cmd.op().to_string(),
        target: match cmd {
            Command::Navigate { url } => Some(url.clone()),
            Command::Snapshot => None,
            Command::Click { r#ref } | Command::Fill { r#ref, .. } => Some(r#ref.clone()),
            // ref 없는 타건의 대상은 포커스된 요소다 — 우리가 아는 것은 키뿐이다.
            Command::PressKey { key, r#ref } => Some(r#ref.clone().unwrap_or_else(|| key.clone())),
            // 기다리는 대상은 문구다. 둘 다 있으면 나타나기(`text`)를 먼저 말한다.
            Command::WaitFor { text, gone, .. } => text.clone().or_else(|| gone.clone()),
            Command::Console { .. } => None,
        },
        changed: None,
        url: url.to_string(),
        controllable,
    }
}

/// 내가 찍은 표시가 아직 최신이면 그 뒤로 새 명령이 없었다는 뜻이다.
pub(crate) fn is_still_idle(app: &tauri::AppHandle, task_id: i64, mark: Instant) -> bool {
    lock_last(&app.state::<AppState>()).get(&task_id).copied() == Some(mark)
}

pub(crate) fn touch(app: &tauri::AppHandle, task_id: i64) -> Instant {
    let mark = Instant::now();
    lock_last(&app.state::<AppState>()).insert(task_id, mark);
    mark
}

fn lock_last(state: &AppState) -> MutexGuard<'_, HashMap<i64, Instant>> {
    state
        .control_last
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_of(cmd: &Command) -> Option<String> {
        control_event(42, true, cmd, "http://localhost:3000/", true).target
    }

    #[test]
    fn action_targets_are_the_ref_and_key_presses_fall_back_to_the_key() {
        let click = Command::Click {
            r#ref: "s1e3".into(),
        };
        let fill = Command::Fill {
            r#ref: "s1e4".into(),
            text: "hi".into(),
        };
        let focused = Command::PressKey {
            key: "Enter".into(),
            r#ref: None,
        };
        assert_eq!(target_of(&click).as_deref(), Some("s1e3"));
        assert_eq!(target_of(&fill).as_deref(), Some("s1e4"));
        assert_eq!(target_of(&focused).as_deref(), Some("Enter"));
        assert_eq!(target_of(&Command::Snapshot), None);
    }

    #[test]
    fn changed_is_only_set_on_the_event_that_carries_a_result() {
        let event = control_event(42, false, &Command::Snapshot, "http://localhost:3000/", true);
        assert_eq!(event.changed, None);
        assert_eq!(event.with_changed(Some(true)).changed, Some(true));
    }
}
