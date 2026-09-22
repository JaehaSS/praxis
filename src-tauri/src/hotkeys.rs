//! 글로벌 단축키 단일 디스패처(Plan 0043 DR-1).
//!
//! `tauri-plugin-global-shortcut` 은 앱당 한 번만 등록되고 핸들러도 전역 하나다.
//! 그래서 "어느 단축키가 눌렸는지" 판별은 여기서 한 번만 하고, 각 기능 모듈은
//! 자기 동작만 갖는다. 기능 핸들러끼리 서로의 분기를 품으면 역방향 의존이 생긴다.
//!
//! 지금 소비자는 음성 하나뿐이지만 디스패처는 남긴다 — 플러그인이 핸들러를 하나만
//! 허용하므로, 두 번째 기능이 붙는 순간 어차피 여기가 갈림길이 된다.

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            // 자기 핫키가 아니면 voice 내부에서 조용히 무시한다.
            crate::voice::on_shortcut(app, shortcut, event.state());
        })
        .build()
}
