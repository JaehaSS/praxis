mod commands;
mod open_path;
mod registry;
mod shell;
mod shell_commands;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub use commands::*;
pub use open_path::*;
pub use registry::{ProjectEditorInfo, ProjectLaunch};
use registry::ProjectRegistry;
use shell::ProjectShell;
pub use shell_commands::*;

#[derive(Default)]
pub struct ProjectEditorState {
    registry: Mutex<ProjectRegistry>,
    shells: Mutex<HashMap<String, Arc<Mutex<ProjectShell>>>>,
    write_guard: Mutex<()>,
    next_session: AtomicU64,
}

impl ProjectEditorState {
    fn root_for_window(&self, label: &str) -> Result<PathBuf, String> {
        let root = self
            .registry
            .lock()
            .unwrap()
            .root_for_label(label)
            .ok_or("등록되지 않은 프로젝트 창입니다")?;
        let current = root.canonicalize().map_err(|error| error.to_string())?;
        if current == root && current.is_dir() {
            Ok(root)
        } else {
            Err("프로젝트 루트가 변경되었거나 접근할 수 없습니다".into())
        }
    }

    fn next_session(&self) -> u64 {
        self.next_session.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// 창 label이 물고 있는 루트(표시 경로). 창이 닫혔음을 알릴 때 무엇이 닫혔는지 싣는다.
pub fn root_of_window(state: &ProjectEditorState, label: &str) -> Option<String> {
    let root = state.registry.lock().unwrap().root_for_label(label)?;
    Some(registry::display_root(&root))
}

pub fn cleanup_window(state: &ProjectEditorState, label: &str) {
    let opening = state.registry.lock().unwrap().remove_label(label);
    if opening.is_none() && !state.shells.lock().unwrap().contains_key(label) {
        return;
    }
    if let Some(opening) = opening {
        opening.complete(Err("프로젝트 창이 닫혔습니다".into()));
    }
    if let Some(shell) = state.shells.lock().unwrap().remove(label) {
        shell.lock().unwrap().terminate();
    }
}

pub fn cleanup_all(state: &ProjectEditorState) {
    // Closing main need not destroy other windows. Their roots stay registered
    // until their own Destroyed event, so editing and shell restart remain usable.
    let shells: Vec<_> = state
        .shells
        .lock()
        .unwrap()
        .drain()
        .map(|(_, shell)| shell)
        .collect();
    for shell in shells {
        shell.lock().unwrap().terminate();
    }
}

fn insert_shell_if_registered(
    state: &ProjectEditorState,
    label: &str,
    shell: Arc<Mutex<ProjectShell>>,
) -> Option<u64> {
    let registry = state.registry.lock().unwrap();
    registry.root_for_label(label)?;
    let mut shells = state.shells.lock().unwrap();
    if let Some(current) = shells.get(label) {
        let current = current.lock().unwrap();
        if !current.exited() {
            return Some(current.session);
        }
    }
    let session = shell.lock().unwrap().session;
    shells.insert(label.to_string(), shell);
    Some(session)
}
