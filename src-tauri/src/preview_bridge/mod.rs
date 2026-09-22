mod control;
pub mod mcp;
mod model;
mod probe;
mod registry;
mod validation;

use std::io::Write;

use tauri::{plugin::TauriPlugin, Manager, Runtime, State, Webview};

pub use model::{
    new_command_id, new_session_id, random_hex_id, CancelReason, PendingAction, RejectReason,
    ResultEnvelope, SessionRegistration, SubmitOutcome, MAX_RESULT_BYTES, PREVIEW_PROBE_TASK_ID,
    PREVIEW_PROBE_WEBVIEW_LABEL,
};
pub use registry::PreviewBridge;
pub use validation::{sha256_hex, validate_preview_probe_url};

pub fn tauri_plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_with(PreviewBridge::new())
}

pub fn tauri_plugin_with<R: Runtime>(bridge: PreviewBridge) -> TauriPlugin<R> {
    tauri::plugin::Builder::<R>::new("preview-bridge")
        .setup(move |app, _api| {
            app.manage(bridge);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![submit_result])
        .build()
}

#[tauri::command]
fn submit_result<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, PreviewBridge>,
    result: ResultEnvelope,
) -> Result<SubmitOutcome, RejectReason> {
    let outcome = bridge.submit(webview.label(), result.clone())?;
    emit_probe_evidence(&webview, &bridge, &result);
    Ok(outcome)
}

fn emit_probe_evidence<R: Runtime>(
    webview: &Webview<R>,
    bridge: &PreviewBridge,
    result: &ResultEnvelope,
) {
    if std::env::var_os("PRAXIS_PREVIEW_PROBE_URL").is_none()
        || !bridge.is_completed_probe(webview.label(), result)
    {
        return;
    }
    let evidence = serde_json::json!({
        "transport": "ipc",
        "elapsedMs": bridge.probe_elapsed_ms().unwrap_or_default(),
        "payloadBytes": result.body.len(),
        "sha256": result.sha256,
        "taskId": result.task_id,
        "webviewLabel": webview.label(),
        "commandId": result.command_id,
        "aclDenied": true,
        "fallback": { "active": false, "executed": false }
    });
    println!("{evidence}");
    let _ = std::io::stdout().flush();
    webview.app_handle().exit(0);
}
