use std::path::{Component, Path, PathBuf};

use tauri::{AppHandle, Manager, Runtime, State, Webview};
use tauri_plugin_opener::OpenerExt;

use super::AppState;

#[tauri::command]
pub async fn open_local_file<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
    state: State<'_, AppState>,
    id: i64,
    path: String,
) -> Result<(), String> {
    require_allowed_webview(webview.label())?;
    reject_invalid_input(&path)?;
    let pool = super::pool_of(&state).map_err(io_error)?;
    let task_root = super::worktree_root(&pool, id).await.map_err(denied)?;
    let home_root = app.path().home_dir().map_err(io_error)?;
    tauri::async_runtime::spawn_blocking(move || {
        open_verified(&path, &task_root, &home_root, |file| {
            let file = file
                .to_str()
                .ok_or_else(|| "검증한 파일 경로를 UTF-8로 표현할 수 없습니다".to_string())?;
            app.opener()
                .open_path(file, None::<&str>)
                .map_err(|error| error.to_string())
        })
    })
    .await
    .map_err(|error| io_error(error))?
}

#[tauri::command]
pub async fn read_local_file<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
    state: State<'_, AppState>,
    id: i64,
    path: String,
) -> Result<crate::fsapi::FileContent, String> {
    require_allowed_webview(webview.label())?;
    reject_invalid_input(&path)?;
    let pool = super::pool_of(&state).map_err(io_error)?;
    let task_root = super::worktree_root(&pool, id).await.map_err(denied)?;
    let home_root = app.path().home_dir().map_err(io_error)?;
    tauri::async_runtime::spawn_blocking(move || read_verified(&path, &task_root, &home_root))
        .await
        .map_err(io_error)?
}

fn require_allowed_webview(label: &str) -> Result<(), String> {
    if matches!(label, "main" | "editor") {
        return Ok(());
    }
    Err(denied("메인 또는 에디터 창에서만 파일을 열 수 있습니다"))
}

fn open_verified(
    path: &str,
    task_root: &Path,
    home_root: &Path,
    opener: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let (root, relative) = select_root(path, task_root, home_root)?;
    let file = validate_file(&root, &relative)?;
    opener(&file).map_err(open_failed)
}

fn read_verified(
    path: &str,
    task_root: &Path,
    home_root: &Path,
) -> Result<crate::fsapi::FileContent, String> {
    let (root, relative) = select_root(path, task_root, home_root)?;
    validate_file(&root, &relative)?;
    let relative = relative
        .to_str()
        .ok_or_else(|| invalid("경로는 UTF-8 문자열이어야 합니다"))?;
    crate::fsapi::read_file(&root, relative).map_err(io_error)
}

fn select_root(
    path: &str,
    task_root: &Path,
    home_root: &Path,
) -> Result<(PathBuf, PathBuf), String> {
    let input = Path::new(path);
    if !input.is_absolute() {
        return Ok((task_root.to_path_buf(), input.to_path_buf()));
    }
    let task_canonical = task_root.canonicalize().map_err(io_error)?;
    if let Some(relative) = relative_to_root(input, task_root, &task_canonical) {
        return Ok((task_root.to_path_buf(), relative));
    }
    let home_canonical = home_root.canonicalize().map_err(io_error)?;
    if let Some(relative) = relative_to_root(input, home_root, &home_canonical) {
        return Ok((home_root.to_path_buf(), relative));
    }
    Err(denied("허용된 경로 밖의 파일은 열 수 없습니다"))
}

fn relative_to_root(input: &Path, raw_root: &Path, canonical_root: &Path) -> Option<PathBuf> {
    input
        .strip_prefix(raw_root)
        .or_else(|_| input.strip_prefix(canonical_root))
        .ok()
        .map(Path::to_path_buf)
}

fn validate_file(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let relative = relative
        .to_str()
        .ok_or_else(|| invalid("경로는 UTF-8 문자열이어야 합니다"))?;
    let canonical_root = root.canonicalize().map_err(io_error)?;
    let candidate = crate::fsapi::safe_join(root, relative).map_err(denied)?;
    let metadata = std::fs::symlink_metadata(&candidate).map_err(file_error)?;
    if !metadata.file_type().is_file() {
        return Err(invalid("일반 파일만 열 수 있습니다"));
    }
    let canonical = candidate.canonicalize().map_err(file_error)?;
    if canonical.starts_with(&canonical_root) {
        return Ok(canonical);
    }
    Err(denied("심볼릭 링크로 허용 경로를 벗어났습니다"))
}

fn reject_invalid_input(path: &str) -> Result<(), String> {
    if path.chars().any(char::is_control) {
        return Err(invalid("제어 문자가 포함된 경로는 열 수 없습니다"));
    }
    if Path::new(path)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(invalid("상위 디렉터리 경로는 열 수 없습니다"));
    }
    Ok(())
}

fn file_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        return not_found("파일을 찾을 수 없습니다");
    }
    io_error(error)
}

fn denied(error: impl std::fmt::Display) -> String {
    format!("local_file_denied: {error}")
}

fn not_found(error: impl std::fmt::Display) -> String {
    format!("local_file_not_found: {error}")
}

fn invalid(error: impl std::fmt::Display) -> String {
    format!("local_file_invalid: {error}")
}

fn io_error(error: impl std::fmt::Display) -> String {
    format!("local_file_io: 파일을 확인하지 못했습니다: {error}")
}

fn open_failed(error: impl std::fmt::Display) -> String {
    format!("local_file_open_failed: 기본 앱으로 파일을 열지 못했습니다: {error}")
}

#[cfg(test)]
#[path = "local_file_tests.rs"]
mod tests;
