use serde::{Deserialize, Serialize};
use tauri::{Emitter, EventTarget, Manager, State, Webview};

use crate::commands::AppState;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolbarIdentity {
    app_epoch: String,
    task_id: i64,
    toolbar_label: String,
    window_generation: u64,
}

#[derive(Deserialize)]
struct ToolbarIntent {
    kind: String,
    action: Option<String>,
    height: Option<f64>,
}

#[derive(Deserialize)]
struct ToolbarMessageKind {
    kind: String,
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("preview-workbench")
        .invoke_handler(tauri::generate_handler![relay, publish])
        .build()
}

#[tauri::command]
fn relay(
    webview: Webview,
    state: State<'_, AppState>,
    mut message: serde_json::Value,
) -> Result<(), String> {
    let kind: ToolbarMessageKind = serde_json::from_value(message.clone())
        .map_err(|_| "툴바 요청 형식이 올바르지 않습니다")?;
    if kind.kind == "ready" {
        let identity = identity_for_caller(&state, webview.label())?;
        canonicalize_ready(&mut message, identity)?;
    } else {
        let identity = verify_toolbar(&state, webview.label(), &message)?;
        resize_toolbar(&state, &identity, &message)?;
    }
    webview
        .app_handle()
        .emit_to(
            EventTarget::Webview {
                label: "main".into(),
            },
            "preview-workbench://toolbar-request",
            message,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn publish(
    webview: Webview,
    state: State<'_, AppState>,
    message: serde_json::Value,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("메인 웹뷰만 툴바 상태를 보낼 수 있습니다".into());
    }
    let identity: ToolbarIdentity = serde_json::from_value(message.clone())
        .map_err(|_| "툴바 상태의 식별자가 올바르지 않습니다")?;
    let toolbar = toolbar_of(&state, &identity)?;
    webview
        .app_handle()
        .emit_to(
            EventTarget::Webview {
                label: toolbar.label().into(),
            },
            "preview-workbench://toolbar-state",
            message,
        )
        .map_err(|error| error.to_string())
}

fn canonicalize_ready(
    message: &mut serde_json::Value,
    identity: ToolbarIdentity,
) -> Result<(), String> {
    let message = message
        .as_object_mut()
        .ok_or("툴바 준비 요청 형식이 올바르지 않습니다")?;
    message.insert("appEpoch".into(), serde_json::json!(identity.app_epoch));
    message.insert("taskId".into(), serde_json::json!(identity.task_id));
    message.insert(
        "toolbarLabel".into(),
        serde_json::json!(identity.toolbar_label),
    );
    message.insert(
        "windowGeneration".into(),
        serde_json::json!(identity.window_generation),
    );
    Ok(())
}

fn identity_for_caller(state: &AppState, caller: &str) -> Result<ToolbarIdentity, String> {
    let handles = state
        .designmode_webviews
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (task_id, handle) = handles
        .iter()
        .find(|(_, handle)| {
            handle
                .toolbar
                .as_ref()
                .is_some_and(|toolbar| toolbar.label() == caller)
        })
        .ok_or("툴바 작업이 일치하지 않습니다")?;
    let identity = ToolbarIdentity {
        app_epoch: state.preview_workbench.app_epoch().to_string(),
        task_id: *task_id,
        toolbar_label: caller.into(),
        window_generation: handle.generation,
    };
    ready_identity(caller, Some(identity))
}

fn ready_identity(
    caller: &str,
    identity: Option<ToolbarIdentity>,
) -> Result<ToolbarIdentity, String> {
    let identity = identity.ok_or("툴바 작업이 일치하지 않습니다")?;
    if identity.toolbar_label != caller {
        return Err("툴바 호출자가 일치하지 않습니다".into());
    }
    Ok(identity)
}

fn verify_toolbar(
    state: &AppState,
    caller: &str,
    message: &serde_json::Value,
) -> Result<ToolbarIdentity, String> {
    let identity: ToolbarIdentity = serde_json::from_value(message.clone())
        .map_err(|_| "툴바 요청의 식별자가 올바르지 않습니다")?;
    if identity.toolbar_label != caller {
        return Err("툴바 호출자가 일치하지 않습니다".into());
    }
    let _ = toolbar_of(state, &identity)?;
    Ok(identity)
}

fn resize_toolbar(
    state: &AppState,
    identity: &ToolbarIdentity,
    message: &serde_json::Value,
) -> Result<(), String> {
    let intent: ToolbarIntent = serde_json::from_value(message.clone()).unwrap_or(ToolbarIntent {
        kind: String::new(),
        action: None,
        height: None,
    });
    if intent.kind != "intent" || intent.action.as_deref() != Some("resize") {
        return Ok(());
    }
    let height = intent.height.ok_or("툴바 높이가 없습니다")?;
    crate::commands::set_window_toolbar_height(
        state,
        identity.task_id,
        &identity.toolbar_label,
        identity.window_generation,
        height,
    )
}

fn toolbar_of(state: &AppState, identity: &ToolbarIdentity) -> Result<Webview, String> {
    if identity.app_epoch != state.preview_workbench.app_epoch() {
        return Err("툴바 앱 세대가 만료되었습니다".into());
    }
    let handles = state
        .designmode_webviews
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let handle = handles
        .get(&identity.task_id)
        .ok_or("프리뷰가 닫혔습니다")?;
    if handle.generation != identity.window_generation {
        return Err("툴바 창 세대가 만료되었습니다".into());
    }
    let toolbar = handle.toolbar.as_ref().ok_or("별도 창 툴바가 없습니다")?;
    if toolbar.label() != identity.toolbar_label {
        return Err("툴바 작업이 일치하지 않습니다".into());
    }
    Ok(toolbar.clone())
}

#[cfg(test)]
mod plugin_tests;
