//! 별도 프로세스·임시 DB에서 실제 WKWebView 자동 열기 경로를 검증한다.
use super::TauriDispatcher;
use crate::commands::{close_preview_surface, AppState};
use crate::preview_bridge::mcp::{Command, DispatchError, Dispatcher};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use tauri::Manager;

pub fn start(app: &tauri::App, url: &str, root: PathBuf) -> Result<(), String> {
    crate::preview_bridge::validate_preview_probe_url(url)?;
    if !root.is_dir() {
        return Err("probe directory missing".into());
    }
    let app = app.handle().clone();
    let url = url.to_string();
    tauri::async_runtime::spawn(async move {
        let result = tokio::time::timeout(Duration::from_secs(55), run(&app, &url, root)).await;
        let result = result.unwrap_or_else(|_| Err("probe timeout".into()));
        match result {
            Ok(evidence) => {
                println!("{evidence}");
                app.exit(0);
            }
            Err(error) => {
                eprintln!("auto-open probe failed: {error}");
                app.exit(1);
            }
        }
    });
    Ok(())
}

async fn run(app: &tauri::AppHandle, url: &str, root: PathBuf) -> Result<Value, String> {
    let pool = crate::db::init_pool(root.join("probe.sqlite").to_str().ok_or("probe path")?)
        .await
        .map_err(|error| error.to_string())?;
    let state = app.state::<AppState>();
    *state.pool.lock().unwrap() = Some(pool.clone());
    let path = root.to_str().ok_or("probe path")?;
    let id = crate::db::insert_task(
        &pool,
        path,
        "probe",
        "main",
        path,
        "probe",
        Some("codex"),
        None,
        "conversation",
        1,
    )
    .await
    .map_err(|error| error.to_string())?;
    let dispatcher = TauriDispatcher {
        app: app.clone(),
        pool,
    };
    let token = state.control_tokens.issue(id, "probe")?;
    let navigate = || Command::Navigate {
        url: url.to_string(),
    };
    ensure(
        matches!(
            dispatcher.dispatch(id, Command::Snapshot).await,
            Err(DispatchError::NoPreview)
        ),
        "initial snapshot must report no_preview",
    )?;
    ensure(
        matches!(
            dispatcher
                .dispatch(
                    id,
                    Command::Navigate {
                        url: "https://example.com".into()
                    }
                )
                .await,
            Err(DispatchError::InvalidUrl(_))
        ),
        "external URL was accepted",
    )?;
    let (first, duplicate) = tokio::join!(
        dispatcher.dispatch(id, navigate()),
        dispatcher.dispatch(id, navigate())
    );
    let successful = [&first, &duplicate]
        .into_iter()
        .filter(|result| result.is_ok())
        .count();
    ensure(
        successful == 1,
        "concurrent opens did not reserve a single slot",
    )?;
    ensure(
        matches!(first, Err(DispatchError::Busy)) || matches!(duplicate, Err(DispatchError::Busy)),
        "duplicate was not busy",
    )?;
    let opened: Value = serde_json::from_str(
        first
            .as_ref()
            .or(duplicate.as_ref())
            .map_err(|error| error.as_str())?,
    )
    .map_err(|error| error.to_string())?;
    ensure(
        opened["opened"] == true && opened["status"] == "navigation_started",
        "wrong open receipt",
    )?;
    let snapshot = snapshot_ready(&dispatcher, id).await?;
    let text = snapshot["snapshot"]["text"]
        .as_str()
        .ok_or("missing snapshot")?;
    let button = text
        .lines()
        .find(|line| line.contains("button") && line.contains("Activate test"))
        .ok_or("button missing")?;
    let reference = button
        .split("[ref=")
        .nth(1)
        .and_then(|value| value.split(']').next())
        .ok_or_else(|| format!("ref missing: {button}"))?;
    let clicked = call(
        &dispatcher,
        id,
        Command::Click {
            r#ref: reference.into(),
        },
    )
    .await?;
    ensure(
        clicked["ok"] == true
            && clicked["changed"] == true
            && clicked.to_string().contains("Activated successfully"),
        "click failed",
    )?;
    let original = state
        .designmode_webviews
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or("window missing")?;
    ensure(
        original
            .window
            .as_ref()
            .ok_or("no window")?
            .is_visible()
            .map_err(|error| error.to_string())?,
        "window not visible",
    )?;
    original
        .window
        .as_ref()
        .unwrap()
        .hide()
        .map_err(|error| error.to_string())?;
    let reused = call(&dispatcher, id, navigate()).await?;
    ensure(
        original
            .window
            .as_ref()
            .unwrap()
            .is_visible()
            .map_err(|error| error.to_string())?,
        "reuse did not reveal hidden window",
    )?;
    ensure(reused["opened"] == false, "existing preview was recreated")?;
    ensure(
        state
            .designmode_webviews
            .lock()
            .unwrap()
            .get(&id)
            .unwrap()
            .generation
            == original.generation,
        "window identity changed",
    )?;
    original
        .webview
        .navigate(tauri::Url::parse("data:text/html,external-origin").unwrap())
        .map_err(|error| error.to_string())?;
    for _ in 0..40 {
        if original
            .webview
            .url()
            .map_err(|error| error.to_string())?
            .scheme()
            == "data"
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    ensure(
        matches!(
            dispatcher.dispatch(id, navigate()).await,
            Err(DispatchError::NotControllableOrigin)
        ),
        "existing external origin was bypassed",
    )?;
    original
        .webview
        .navigate(tauri::Url::parse(url).unwrap())
        .map_err(|error| error.to_string())?;
    snapshot_ready(&dispatcher, id).await?;
    state.preview_bridge.take_over(id);
    ensure(
        matches!(
            dispatcher.dispatch(id, navigate()).await,
            Err(DispatchError::TakenOver)
        ),
        "takeover bypassed",
    )?;
    ensure(state.preview_bridge.is_taken_over(id), "takeover cleared")?;
    state.preview_bridge.release(id);
    close_preview_surface(&state, id);
    ensure(
        state.control_tokens.task_for(&token) == Some(id),
        "surface close revoked MCP token",
    )?;
    let reopened = call(&dispatcher, id, navigate()).await?;
    ensure(
        reopened["opened"] == true,
        "closed preview was not reopened",
    )?;
    snapshot_ready(&dispatcher, id).await?;
    ensure(
        state.designmode_webviews.lock().unwrap().len() == 1,
        "duplicate preview handle",
    )?;
    ensure(
        matches!(
            dispatcher.dispatch(id + 100, Command::Snapshot).await,
            Err(DispatchError::NoPreview)
        ),
        "task isolation failed",
    )?;
    close_preview_surface(&state, id);
    ensure(
        state.designmode_webviews.lock().unwrap().is_empty(),
        "preview cleanup failed",
    )?;
    Ok(
        json!({"probe":"preview-auto-open", "firstOpen":true, "duplicateBusy":true, "snapshot":true, "click":true,
        "visible":true, "reuse":true, "takeoverPreserved":true, "tokenPreserved":true, "reopen":true, "taskIsolation":true, "externalUrlRejected":true, "externalOriginRejected":true, "cleanup":true}),
    )
}

fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

async fn call(dispatcher: &TauriDispatcher, id: i64, command: Command) -> Result<Value, String> {
    let body = dispatcher
        .dispatch(id, command)
        .await
        .map_err(|error| error.as_str().to_string())?;
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

async fn snapshot_ready(dispatcher: &TauriDispatcher, id: i64) -> Result<Value, String> {
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        match call(dispatcher, id, Command::Snapshot).await {
            Ok(value) if value.to_string().contains("Activate test") => return Ok(value),
            Ok(_) => {}
            Err(error) if error == "stale" || error == "timeout" => {}
            Err(error) => return Err(error),
        }
    }
    Err("page never became ready".into())
}
