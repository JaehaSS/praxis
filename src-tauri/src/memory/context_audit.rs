//! Safe context visibility for the selected host's task and memory store.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sqlx::SqlitePool;

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Serialize)]
pub struct ContextFile {
    pub role: String,
    pub path: String,
    pub exists: bool,
    pub size: u64,
    pub has_praxis_block: bool,
}

#[derive(Serialize)]
pub struct VendorContext {
    pub vendor: String,
    pub uncertain: bool,
    pub files: Vec<ContextFile>,
}

#[derive(Serialize)]
pub struct ContextReport {
    pub vendors: Vec<VendorContext>,
    pub injected: Vec<super::InjectedMemory>,
    pub capture_enabled: bool,
    /// 회고는 추출과 독립 스위치다 — 하나의 불린으로는 "캡처가 켜졌나"에 답할 수 없다.
    pub reflect_enabled: bool,
    pub memory_count: i64,
    pub memory_counts: super::context_summary::MemoryContextCounts,
    pub projection: Option<super::context_summary::MemoryProjectionSummary>,
    /// 인용 관측 집계(설계 0048) — 주입 없던 작업은 None. read-only, 자격·랭킹과 무관.
    pub citations: Option<super::citation::CitationCounts>,
}

pub async fn report(
    pool: &SqlitePool,
    task_id: i64,
    home: &Path,
    capture_enabled: bool,
    reflect_enabled: bool,
) -> anyhow::Result<ContextReport> {
    let task = crate::db::get_task(pool, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("작업을 찾을 수 없습니다"))?;
    let project = PathBuf::from(&task.worktree_path);
    let vendors = vendor_matrix(home)
        .into_iter()
        .map(|(vendor, global, project_file, uncertain)| VendorContext {
            vendor: vendor.to_string(),
            uncertain,
            files: vec![
                inspect_scoped("global", home, global.strip_prefix(home).unwrap()),
                inspect_scoped("project", &project, Path::new(project_file)),
            ],
        })
        .collect();
    let diagnostics = super::context_summary::diagnostics(pool, task_id, &task.repo).await?;
    Ok(ContextReport {
        vendors,
        injected: super::injections_for_task(pool, task_id).await?,
        capture_enabled,
        reflect_enabled,
        memory_count: diagnostics.project_count,
        memory_counts: diagnostics.memory_counts,
        projection: diagnostics.projection,
        citations: super::citation::summary(pool, task_id).await?,
    })
}

pub async fn read_file(
    pool: &SqlitePool,
    task_id: i64,
    home: &Path,
    requested: &Path,
) -> anyhow::Result<String> {
    let task = crate::db::get_task(pool, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("작업을 찾을 수 없습니다"))?;
    let project = PathBuf::from(task.worktree_path);
    let Some((root, relative)) = allowed_locator(requested, home, &project) else {
        anyhow::bail!("허용되지 않은 경로입니다");
    };
    read_regular_utf8(root, &relative)
}

pub fn vendor_matrix(home: &Path) -> Vec<(&'static str, PathBuf, &'static str, bool)> {
    vec![
        ("claude", home.join(".claude/CLAUDE.md"), "CLAUDE.md", false),
        ("codex", home.join(".codex/AGENTS.md"), "AGENTS.md", false),
        ("gemini", home.join(".gemini/GEMINI.md"), "GEMINI.md", false),
        ("agy", home.join(".gemini/GEMINI.md"), "GEMINI.md", true),
    ]
}

pub fn allowed(path: &Path, home: &Path, project: &Path) -> bool {
    allowed_locator(path, home, project).is_some()
}

fn allowed_locator<'a>(
    path: &Path,
    home: &'a Path,
    project: &'a Path,
) -> Option<(&'a Path, PathBuf)> {
    vendor_matrix(home)
        .into_iter()
        .find_map(|(_, global, project_file, _)| {
            if global == path {
                return Some((home, global.strip_prefix(home).ok()?.to_path_buf()));
            }
            (project.join(project_file) == path).then(|| (project, PathBuf::from(project_file)))
        })
}

pub fn inspect(role: &str, path: &Path) -> ContextFile {
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let relative = path.file_name().map(Path::new).unwrap_or(path);
    inspect_scoped(role, root, relative)
}

fn inspect_scoped(role: &str, root: &Path, relative: &Path) -> ContextFile {
    let path = root.join(relative);
    let metadata = std::fs::symlink_metadata(&path);
    let (exists, size) = metadata
        .as_ref()
        .map(|value| (true, value.len()))
        .unwrap_or((false, 0));
    let has_praxis_block = read_regular_utf8(root, relative)
        .ok()
        .and_then(|text| super::extract_injected_block(&text))
        .is_some();
    ContextFile {
        role: role.to_string(),
        path: path.to_string_lossy().into_owned(),
        exists,
        size,
        has_praxis_block,
    }
}

fn read_regular_utf8(root: &Path, relative: &Path) -> anyhow::Result<String> {
    let relative = relative
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("context path is not valid UTF-8"))?;
    let mut file = crate::evidence::scoped_file::open(root, relative)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        anyhow::bail!("파일이 아닙니다");
    }
    if metadata.len() > MAX_FILE_BYTES {
        anyhow::bail!("파일이 너무 큽니다");
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    std::io::Read::by_ref(&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        anyhow::bail!("파일이 너무 큽니다");
    }
    String::from_utf8(bytes).map_err(Into::into)
}
