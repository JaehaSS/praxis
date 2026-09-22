//! 팝아웃된 에디터 창의 상태 — 창 기하와 열린 파일 목록.
//!
//! 창은 `tauri.conf.json`에 `visible: false`로 만들어 두고 팝아웃 때 보인다. 매번 생성하면
//! 첫 렌더가 느리다(어항과 같은 방식).
//!
//! 여기에는 순수 로직만 둔다. `#[tauri::command]`는 `commands.rs`에 있다 — `bench`·`rewind`와
//! 같은 구조다.

use serde::{Deserialize, Serialize};

pub const WINDOW_LABEL: &str = "editor";

/// 창이 **실제로** 사라졌음을 메인 창에 알리는 이벤트(`editor-window-events.ts`의 `EDITOR_GONE_EVENT`).
///
/// 팝인은 숨김이라 정상 경로에서는 오지 않는다. 세션이 없는 채로 닫혔거나 웹뷰가 죽은 채로
/// 닫혀 Tauri가 창을 destroy했을 때만 온다 — 메인 창의 팝아웃 상태는 `editor://closed`로만
/// 풀리는데 죽은 창은 그것을 보내지 못하므로 Rust가 대신 알린다.
pub const GONE_EVENT: &str = "editor://gone";

const GEOMETRY_KEY: &str = "editor_window_geometry";
const OPEN_FILES_KEY_PREFIX: &str = "editor_window_files_";

/// 창을 잡아 옮길 수 있으려면 이만큼은 모니터 안에 있어야 한다.
/// 타이틀바를 집을 수 없는 창은 사용자가 되찾을 방법이 없다.
const MIN_VISIBLE: i64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// 팝인했다가 다시 팝아웃할 때 이어받을 목록. 세션마다 따로 저장한다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenFilesState {
    pub open_paths: Vec<String>,
    pub active_path: Option<String>,
}

pub fn geometry_key() -> &'static str {
    GEOMETRY_KEY
}

/// 창은 하나지만 세션마다 열린 파일이 다르다.
pub fn open_files_key(task_id: i64) -> String {
    format!("{OPEN_FILES_KEY_PREFIX}{task_id}")
}

/// 저장된 자리가 지금 화면 구성에서 아직 닿을 수 있는가.
///
/// 두 번째 모니터를 떼면 그 좌표는 어느 화면에도 걸치지 않는다. 그대로 복원하면 창이 보이지
/// 않는 곳에 떠서 되찾을 수 없으므로, 이 판정이 거짓이면 기본 위치로 떨어뜨린다.
pub fn is_reachable(g: &Geometry, monitors: &[Geometry]) -> bool {
    monitors.iter().any(|m| {
        let overlap = |a_start: i64, a_len: i64, b_start: i64, b_len: i64| {
            (a_start + a_len).min(b_start + b_len) - a_start.max(b_start)
        };
        let w = overlap(g.x as i64, g.width as i64, m.x as i64, m.width as i64);
        let h = overlap(g.y as i64, g.height as i64, m.y as i64, m.height as i64);
        w >= MIN_VISIBLE && h >= MIN_VISIBLE
    })
}

#[cfg(test)]
mod window_tests {
    use tauri::Manager;

    fn app_with(
        windows: Vec<tauri::utils::config::WindowConfig>,
    ) -> tauri::App<tauri::test::MockRuntime> {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().app.windows = windows;
        tauri::test::mock_builder().build(context).unwrap()
    }

    /// 창이 destroy된 뒤에도 팝아웃이 되어야 한다 — 설정에서 같은 label로 다시 만든다.
    ///
    /// mock 런타임은 이벤트 루프가 없어 `destroy()`가 매니저에서 창을 빼지 못한다. 그래서
    /// "설정은 있는데 창은 없는" 상태 — destroy 직후와 같은 상태 — 를 부팅 직후로 만든다
    /// (mock 빌더는 설정의 창을 미리 만들지 않는다; 아래 첫 assert가 그 전제를 지킨다).
    #[test]
    fn open_recreates_missing_editor_window_from_config() {
        let app = app_with(vec![tauri::utils::config::WindowConfig {
            label: super::WINDOW_LABEL.into(),
            visible: false,
            ..Default::default()
        }]);
        let handle = app.handle();
        assert!(handle.get_webview_window(super::WINDOW_LABEL).is_none());

        let created = crate::commands::editor_window_or_create(handle).unwrap();
        assert_eq!(created.label(), super::WINDOW_LABEL);
        assert!(handle.get_webview_window(super::WINDOW_LABEL).is_some());
        // 이미 있으면 다시 만들지 않는다.
        let again = crate::commands::editor_window_or_create(handle).unwrap();
        assert_eq!(again.label(), super::WINDOW_LABEL);
        assert_eq!(handle.webview_windows().len(), 1);
    }

    #[test]
    fn open_fails_clearly_without_editor_config() {
        let app = app_with(vec![]);
        let err = crate::commands::editor_window_or_create(app.handle()).unwrap_err();
        assert!(err.contains("설정"), "{err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Geometry = Geometry { x: 0, y: 0, width: 1920, height: 1080 };
    const SECOND: Geometry = Geometry { x: 1920, y: 0, width: 2560, height: 1440 };

    fn at(x: i32, y: i32) -> Geometry {
        Geometry { x, y, width: 1100, height: 800 }
    }

    #[test]
    fn 세션마다_다른_키를_쓴다() {
        assert_eq!(open_files_key(42), "editor_window_files_42");
        assert_ne!(open_files_key(42), open_files_key(43));
    }

    #[test]
    fn 모니터_안에_있으면_닿는다() {
        assert!(is_reachable(&at(100, 100), &[MAIN]));
        assert!(is_reachable(&at(2000, 100), &[MAIN, SECOND]));
    }

    #[test]
    fn 두번째_모니터가_사라지면_닿지_않는다() {
        // 듀얼 모니터에서 쓰던 자리를 노트북만 들고 나갔을 때가 이 경우다.
        assert!(!is_reachable(&at(2000, 100), &[MAIN]));
    }

    #[test]
    fn 걸침이_임계보다_적으면_닿지_않는다() {
        // 타이틀바를 집을 수 없을 만큼만 걸쳐 있으면 되찾을 수 없다.
        assert!(!is_reachable(&at(1870, 100), &[MAIN]));
        assert!(!is_reachable(&at(100, 1030), &[MAIN]));
    }

    #[test]
    fn 모니터가_하나도_없으면_닿지_않는다() {
        assert!(!is_reachable(&at(0, 0), &[]));
    }

    #[test]
    fn 열린_파일_상태는_왕복한다() {
        let state = OpenFilesState {
            open_paths: vec!["src/a.ts".into(), "src/b.ts".into()],
            active_path: Some("src/b.ts".into()),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<OpenFilesState>(&json).unwrap(), state);
    }

    #[test]
    fn 저장된_적이_없으면_빈_상태로_읽는다() {
        assert_eq!(OpenFilesState::default().open_paths.len(), 0);
        assert!(OpenFilesState::default().active_path.is_none());
    }
}
