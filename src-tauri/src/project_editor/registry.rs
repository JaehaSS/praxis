use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::watch;

use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ProjectEditorInfo {
    pub root: String,
    pub label: String,
    /// 창의 첫 셸이 일반 셸 대신 에이전트 CLI로 뜬다 — 창이 터미널을 바로 연다.
    pub launch: bool,
}

/// 창의 PTY가 `default_shell()` 대신 실행할 명령. 창을 만들 때 한 번 정해진다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectLaunch {
    pub bin: String,
    pub args: Vec<String>,
}

pub struct Opening {
    result: watch::Sender<Option<Result<ProjectEditorInfo, String>>>,
}

impl Opening {
    fn new() -> Self {
        Self {
            result: watch::channel(None).0,
        }
    }

    pub fn complete(&self, result: Result<ProjectEditorInfo, String>) {
        self.result.send_replace(Some(result));
    }

    pub async fn wait(&self) -> Result<ProjectEditorInfo, String> {
        let mut result = self.result.subscribe();
        loop {
            if let Some(result) = result.borrow().clone() {
                return result;
            }
            result
                .changed()
                .await
                .map_err(|_| "프로젝트 창 열기가 취소되었습니다".to_string())?;
        }
    }
}

enum Entry {
    Opening {
        label: String,
        opening: Arc<Opening>,
    },
    Ready(ProjectEditorInfo),
}

pub enum OpenAction {
    Focus(ProjectEditorInfo),
    Wait(Arc<Opening>),
    Create {
        label: String,
        opening: Arc<Opening>,
    },
}

#[derive(Default)]
pub struct ProjectRegistry {
    entries: HashMap<PathBuf, Entry>,
    labels: HashMap<String, PathBuf>,
    launches: HashMap<String, ProjectLaunch>,
    next_label: u64,
}

impl ProjectRegistry {
    /// 같은 root의 창이 이미 있으면 기존 창을 focus하고 `launch`는 버린다.
    pub fn begin_open(&mut self, root: PathBuf, launch: Option<ProjectLaunch>) -> OpenAction {
        if let Some(entry) = self.entries.get(&root) {
            return match entry {
                Entry::Ready(info) => OpenAction::Focus(info.clone()),
                Entry::Opening { opening, .. } => OpenAction::Wait(opening.clone()),
            };
        }
        self.next_label += 1;
        let label = format!("project-editor-{}", self.next_label);
        let opening = Arc::new(Opening::new());
        self.labels.insert(label.clone(), root.clone());
        if let Some(launch) = launch {
            self.launches.insert(label.clone(), launch);
        }
        self.entries.insert(
            root,
            Entry::Opening {
                label: label.clone(),
                opening: opening.clone(),
            },
        );
        OpenAction::Create { label, opening }
    }

    pub fn complete_open(
        &mut self,
        root: &Path,
        opening: &Arc<Opening>,
    ) -> Option<ProjectEditorInfo> {
        let Entry::Opening {
            label,
            opening: current,
        } = self.entries.get(root)?
        else {
            return None;
        };
        if !Arc::ptr_eq(current, opening) {
            return None;
        }
        let info = ProjectEditorInfo {
            root: display_root(root),
            label: label.clone(),
            launch: self.launches.contains_key(label),
        };
        self.entries
            .insert(root.to_path_buf(), Entry::Ready(info.clone()));
        Some(info)
    }

    pub fn fail_open(&mut self, root: &Path, opening: &Arc<Opening>) {
        let Some(Entry::Opening {
            label,
            opening: current,
        }) = self.entries.get(root)
        else {
            return;
        };
        if !Arc::ptr_eq(current, opening) {
            return;
        }
        let label = label.clone();
        self.entries.remove(root);
        self.labels.remove(&label);
        self.launches.remove(&label);
    }

    pub fn root_for_label(&self, label: &str) -> Option<PathBuf> {
        self.labels.get(label).cloned()
    }

    pub fn launch_for_label(&self, label: &str) -> Option<ProjectLaunch> {
        self.launches.get(label).cloned()
    }

    pub fn remove_label(&mut self, label: &str) -> Option<Arc<Opening>> {
        let root = self.labels.remove(label)?;
        self.launches.remove(label);
        let opening = match self.entries.get(&root) {
            Some(Entry::Ready(info)) if info.label == label => None,
            Some(Entry::Opening {
                label: entry_label,
                opening,
            }) if entry_label == label => Some(opening.clone()),
            _ => return None,
        };
        self.entries.remove(&root);
        opening
    }
}

pub fn canonical_root(root: &str) -> Result<PathBuf, String> {
    let root = Path::new(root)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !root.is_dir() {
        return Err("프로젝트 루트가 디렉터리가 아닙니다".into());
    }
    Ok(root)
}

pub fn display_root(root: &Path) -> String {
    crate::fsapi::display_path(root)
}
