use super::*;
use crate::preview_bridge::mcp::{is_loopback_origin, DispatchError};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewState {
    pub task_id: i64,
    pub url: String,
    pub mode: designmode::PreviewMode,
    pub generation: u64,
}

/// 마운트 전에 열린 창과 놓친 이벤트도 복원할 수 있는 네이티브 상태.
#[tauri::command]
pub fn designmode_state(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Option<PreviewState>, String> {
    state_of(&state, id)
}

fn state_of(state: &AppState, id: i64) -> Result<Option<PreviewState>, String> {
    let handle = state.designmode_webviews.lock().unwrap().get(&id).cloned();
    handle
        .map(|handle| {
            Ok(PreviewState {
                task_id: id,
                url: handle
                    .webview
                    .url()
                    .map_err(|error| error.to_string())?
                    .to_string(),
                mode: handle.mode,
                generation: handle.generation,
            })
        })
        .transpose()
}

pub async fn open_agent_preview(
    app: &AppHandle,
    state: &AppState,
    id: i64,
    url: &str,
) -> Result<bool, DispatchError> {
    let parsed = crate::preview_bridge::validate_preview_probe_url(url)
        .map_err(DispatchError::InvalidUrl)?;
    let generation = next_preview_generation();
    let opening = state.preview_openings.begin(id, generation)?;
    let handle = state.designmode_webviews.lock().unwrap().get(&id).cloned();
    let opened = handle.is_none();
    if let Some(handle) = handle {
        let current = handle.webview.url().map_err(|_| DispatchError::NoPreview)?;
        if !is_loopback_origin(&current) {
            return Err(DispatchError::NotControllableOrigin);
        }
        state.preview_bridge.prepare_agent_navigation(id)?;
        if let Err(error) = handle.webview.navigate(parsed) {
            let _ = state.preview_bridge.cancel_prepared_navigation(id);
            return Err(DispatchError::NavigationFailed(error.to_string()));
        }
        if let Some(window) = handle.window {
            window
                .unminimize()
                .map_err(|error| DispatchError::NavigationFailed(error.to_string()))?;
            window
                .show()
                .map_err(|error| DispatchError::NavigationFailed(error.to_string()))?;
        }
        // Inline은 메인 UI가 작업과 패널을 선택하고 최신 bounds로 표시한다.
    } else {
        let pool = pool_of(state).map_err(DispatchError::NavigationFailed)?;
        let task = db::get_task(&pool, id)
            .await
            .map_err(|error| DispatchError::NavigationFailed(error.to_string()))?
            .ok_or(DispatchError::NoPreview)?;
        if matches!(
            task.state.as_str(),
            db::state::DONE | db::state::DISCARDED | db::state::FINALIZING
        ) {
            return Err(DispatchError::NoPreview);
        }
        let worktree = PathBuf::from(task.worktree_path);
        if !worktree.is_dir() {
            return Err(DispatchError::NavigationFailed(
                "작업 폴더가 없습니다".into(),
            ));
        }
        opening
            .publish(|| Ok(()))
            .map_err(DispatchError::NavigationFailed)?;
        // UI 패널이 아직 없으므로 기본 창을 메인 창 옆에 배치한다.
        let bounds = DesignBounds {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
        create_preview(
            app,
            state,
            id,
            parsed,
            bounds,
            designmode::PreviewMode::Window,
            worktree,
            &opening,
        )
        .map_err(DispatchError::NavigationFailed)?;
    }
    let preview = state_of(state, id)
        .map_err(DispatchError::NavigationFailed)?
        .ok_or(DispatchError::NoPreview)?;
    app.emit("designmode://activated", preview)
        .map_err(|error| DispatchError::NavigationFailed(error.to_string()))?;
    Ok(opened)
}

pub(super) fn page_loaded(webview: tauri::Webview, payload: tauri::webview::PageLoadPayload<'_>) {
    if payload.event() != tauri::webview::PageLoadEvent::Finished {
        return;
    }
    let state = webview.app_handle().state::<AppState>();
    let id = state
        .designmode_webviews
        .lock()
        .unwrap()
        .iter()
        .find(|(_, handle)| handle.webview.label() == webview.label())
        .map(|(id, _)| *id);
    if let Some(id) = id {
        let _ = webview.app_handle().emit("designmode://changed", id);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn closing_surface_preserves_token_but_finishing_task_revokes_it() {
        let state = crate::commands::AppState::default();
        let token = state.control_tokens.issue(7, "test-turn").unwrap();
        crate::commands::close_preview_surface(&state, 7);
        assert_eq!(state.control_tokens.task_for(&token), Some(7));
        crate::commands::close_designmode_webview_of(&state, 7);
        assert_eq!(state.control_tokens.task_for(&token), None);
    }
}
