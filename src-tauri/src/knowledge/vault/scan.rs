use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool};

use super::files::{import_file, ImportRequest};
use super::scope::ScopeRequest;
use super::wiki;

const MAX_FILES: usize = 10_000;
const MAX_TEXT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_INDEXED_TEXT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct VaultScanResult {
    pub indexed: usize,
    pub skipped: usize,
    pub partial: bool,
    pub warnings: Vec<String>,
    pub remaining_path: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScanLimits {
    pub max_files: usize,
    pub max_text_bytes: u64,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_files: MAX_FILES,
            max_text_bytes: MAX_TEXT_BYTES,
        }
    }
}

pub async fn scan_vault(
    pool: &SqlitePool,
    vault_id: &str,
    scope: ScopeRequest,
    now: i64,
) -> anyhow::Result<VaultScanResult> {
    scan_vault_with_exclusions(pool, vault_id, scope, &[], now).await
}

pub async fn scan_vault_with_exclusions(
    pool: &SqlitePool,
    vault_id: &str,
    scope: ScopeRequest,
    exclusions: &[PathBuf],
    now: i64,
) -> anyhow::Result<VaultScanResult> {
    scan_vault_with_limits(
        pool,
        vault_id,
        scope,
        exclusions,
        now,
        ScanLimits::default(),
    )
    .await
}

pub(crate) async fn scan_vault_with_limits(
    pool: &SqlitePool,
    vault_id: &str,
    scope: ScopeRequest,
    exclusions: &[PathBuf],
    now: i64,
    limits: ScanLimits,
) -> anyhow::Result<VaultScanResult> {
    let _admission = super::shared_admission(pool).await?;
    // 위키 폴더는 설정이다. 스캔 한 번 동안은 고정된 값을 쓴다 — 파일마다 다시
    // 읽으면 도중에 바뀐 설정이 같은 스캔 안에서 갈린 판정을 낳는다.
    let wiki_dir = super::settings::load(pool).await.wiki_dir;
    let vault_root = super::files::vault_root(pool, vault_id).await?;
    let root = vault_root.path.to_string_lossy().into_owned();
    let mut paths = Vec::new();
    let mut result = VaultScanResult {
        indexed: 0,
        skipped: 0,
        partial: false,
        warnings: Vec::new(),
        remaining_path: None,
    };
    let collected = collect(
        Path::new(&root),
        Path::new(&root),
        exclusions,
        limits.max_files,
        &mut paths,
    );
    result.partial = collected.partial;
    result.remaining_path = collected.remaining_path;
    result.warnings.extend(collected.warnings);
    let mut text_bytes = 0;
    let mut seen = HashSet::new();
    for path in paths {
        let relative = relative(&root, &path)?;
        seen.insert(relative.clone());
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                result.skipped += 1;
                result.warnings.push(format!("{relative}: {error}"));
                continue;
            }
        };
        let size = metadata.len();
        if is_indexable_text(&path, size) {
            text_bytes += size;
        }
        if text_bytes > limits.max_text_bytes {
            result.partial = true;
            result.remaining_path = Some(relative);
            result
                .warnings
                .push("eligible text scan limit reached".into());
            break;
        }
        if let Some(document_id) = current_document(pool, vault_id, &relative).await? {
            if wiki::is_wiki_path(&relative, &wiki_dir) {
                match wiki::refresh_wiki_file(pool, &document_id, &vault_root, &relative, now).await
                {
                    Ok(warnings) => result.warnings.extend(warnings),
                    Err(error) => result.warnings.push(format!("{relative}: {error}")),
                }
            } else {
                refresh_current(pool, &document_id, &root, &relative).await?;
            }
            result.skipped += 1;
            continue;
        }
        if known_path(pool, vault_id, &relative).await? {
            result.skipped += 1;
            continue;
        }
        if wiki::is_wiki_path(&relative, &wiki_dir) {
            match wiki::register_wiki_file(pool, vault_id, &vault_root, &relative, &scope, now)
                .await
            {
                Ok(warnings) => {
                    result.indexed += 1;
                    result.warnings.extend(warnings);
                }
                Err(error) => {
                    result.skipped += 1;
                    result.warnings.push(format!("{relative}: {error}"));
                }
            }
            continue;
        }
        let title = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Source")
            .to_owned();
        match import_file(
            pool,
            &ImportRequest {
                vault_id: vault_id.into(),
                source: path,
                title,
                scope: scope.clone(),
            },
            now,
        )
        .await
        {
            Ok(_) => result.indexed += 1,
            Err(error) => {
                result.skipped += 1;
                result.warnings.push(error.to_string());
            }
        }
    }
    if !result.partial {
        mark_unseen_missing(pool, vault_id, &root, &seen, exclusions, now, &mut result).await?;
        super::index::mark_rebuild_complete(pool).await?;
    }
    Ok(result)
}

fn collect(
    root: &Path,
    directory: &Path,
    exclusions: &[PathBuf],
    max_files: usize,
    paths: &mut Vec<PathBuf>,
) -> CollectResult {
    let mut result = CollectResult::default();
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            result.partial = true;
            result.remaining_path = Some(directory.to_string_lossy().into_owned());
            result
                .warnings
                .push(format!("{}: {error}", directory.display()));
            return result;
        }
    };
    for entry in entries {
        if paths.len() >= max_files {
            result.partial = true;
            result.remaining_path = Some(directory.to_string_lossy().into_owned());
            return result;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                result.warnings.push(error.to_string());
                result.partial = true;
                result.remaining_path = Some(directory.to_string_lossy().into_owned());
                continue;
            }
        };
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(error) => {
                result
                    .warnings
                    .push(format!("{}: {error}", entry.path().display()));
                result.partial = true;
                result.remaining_path = Some(entry.path().to_string_lossy().into_owned());
                continue;
            }
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if is_excluded(root, relative, exclusions) {
            continue;
        }
        if kind.is_dir() {
            let nested = collect(root, &path, exclusions, max_files, paths);
            result.warnings.extend(nested.warnings);
            if nested.partial {
                result.partial = true;
                result.remaining_path = nested.remaining_path;
                return result;
            }
        }
        if kind.is_file() {
            if is_root_agent_file(relative) {
                continue;
            }
            paths.push(path);
        }
    }
    result
}

#[derive(Default)]
struct CollectResult {
    partial: bool,
    remaining_path: Option<String>,
    warnings: Vec<String>,
}

pub(crate) fn excluded(path: &Path) -> bool {
    path.components().any(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some(".git" | ".obsidian" | ".trash" | ".ssh" | ".aws" | ".claude")
        )
    }) || path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name == ".env"
                || name.starts_with(".env.")
                // Finder가 폴더마다 남기는 메타데이터 — 자료로 등록되면 창고가
                // 사용자가 만들지 않은 파일로 찬다.
                || name == ".DS_Store"
                || (name.starts_with(".vault-") && name.ends_with(".tmp"))
        })
}

/// 창고 루트 바로 아래의 에이전트 설정 파일. 자료로 등록하지 않는다.
fn is_root_agent_file(relative: &Path) -> bool {
    relative.components().count() == 1
        && matches!(
            relative.to_str(),
            Some("CLAUDE.md" | "AGENTS.md" | "GEMINI.md")
        )
}

fn is_indexable_text(path: &Path, size: u64) -> bool {
    size <= MAX_INDEXED_TEXT_BYTES
        && matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("md" | "MD" | "txt" | "TXT" | "csv" | "CSV")
        )
}

async fn current_document(
    pool: &SqlitePool,
    vault_id: &str,
    relative: &str,
) -> anyhow::Result<Option<String>> {
    sqlx::query_scalar("SELECT d.id FROM vault_revisions r JOIN vault_documents d ON d.id = r.document_id WHERE d.vault_id = ? AND d.current_revision = r.id AND r.relative_path = ?")
        .bind(vault_id)
        .bind(relative)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
}

async fn known_path(pool: &SqlitePool, vault_id: &str, relative: &str) -> anyhow::Result<bool> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM vault_revisions r JOIN vault_documents d ON d.id = r.document_id WHERE d.vault_id = ? AND r.relative_path = ?")
        .bind(vault_id)
        .bind(relative)
        .fetch_optional(pool)
        .await?;
    Ok(exists.is_some())
}

async fn refresh_current(
    pool: &SqlitePool,
    document_id: &str,
    root: &str,
    relative: &str,
) -> anyhow::Result<()> {
    if super::files::verify_revision(pool, &current_revision_id(pool, document_id).await?)
        .await
        .is_ok()
    {
        sqlx::query(
            "UPDATE vault_documents SET state = 'active' WHERE id = ? AND state <> 'archived'",
        )
        .bind(document_id)
        .execute(pool)
        .await?;
        let revision_id = current_revision_id(pool, document_id).await?;
        super::index::index_revision(pool, &revision_id).await?;
        return Ok(());
    }
    let state = if std::fs::metadata(Path::new(root).join(relative)).is_ok() {
        "drifted"
    } else {
        "missing"
    };
    sqlx::query("UPDATE vault_documents SET state = ? WHERE id = ? AND state <> 'archived'")
        .bind(state)
        .bind(document_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM vault_fts WHERE rowid = (SELECT r.rowid FROM vault_revisions r JOIN vault_documents d ON d.current_revision = r.id WHERE d.id = ?)")
        .bind(document_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn current_revision_id(pool: &SqlitePool, document_id: &str) -> anyhow::Result<String> {
    sqlx::query_scalar("SELECT current_revision FROM vault_documents WHERE id = ?")
        .bind(document_id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

async fn mark_unseen_missing(
    pool: &SqlitePool,
    vault_id: &str,
    root: &str,
    seen: &HashSet<String>,
    exclusions: &[PathBuf],
    _now: i64,
    result: &mut VaultScanResult,
) -> anyhow::Result<()> {
    let rows = sqlx::query("SELECT d.id, r.rowid AS revision_rowid, r.relative_path FROM vault_documents d JOIN vault_revisions r ON r.id = d.current_revision WHERE d.vault_id = ? AND d.state <> 'archived'")
        .bind(vault_id)
        .fetch_all(pool)
        .await?;
    for row in rows {
        let relative: String = row.try_get("relative_path")?;
        if seen.contains(&relative)
            || is_excluded(Path::new(root), Path::new(&relative), exclusions)
        {
            continue;
        }
        sqlx::query("UPDATE vault_documents SET state = 'missing' WHERE id = ?")
            .bind(row.try_get::<String, _>("id")?)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM vault_fts WHERE rowid = ?")
            .bind(row.try_get::<i64, _>("revision_rowid")?)
            .execute(pool)
            .await?;
        result.skipped += 1;
    }
    Ok(())
}

fn relative(root: &str, path: &Path) -> anyhow::Result<String> {
    path.strip_prefix(root)
        .map(|value| value.to_string_lossy().into_owned())
        .map_err(Into::into)
}

fn is_excluded(root: &Path, relative: &Path, exclusions: &[PathBuf]) -> bool {
    excluded(relative)
        || exclusions.iter().any(|path| {
            let path = path.strip_prefix(root).unwrap_or(path);
            relative.starts_with(path)
        })
}

#[cfg(test)]
mod scan_tests {
    use super::*;

    #[test]
    fn traversal_error_is_partial_and_vault_temps_are_excluded() {
        let root = crate::testtmp::dir();
        let file = root.join("not-a-directory");
        std::fs::write(&file, "text").unwrap();
        let mut paths = Vec::new();
        let result = collect(&root, &file, &[], 10, &mut paths);
        assert!(result.partial);
        assert_eq!(result.remaining_path.as_deref(), file.to_str());
        assert!(excluded(Path::new(".vault-operation.tmp")));
        // Finder 메타데이터는 어느 깊이에 있든 자료가 아니다.
        assert!(excluded(Path::new(".DS_Store")));
        assert!(excluded(Path::new("notes/팀/.DS_Store")));
        assert!(!excluded(Path::new("notes/DS_Store.md")));
    }
}
