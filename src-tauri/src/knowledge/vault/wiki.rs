//! `wiki/` 아래 마크다운을 정본으로 다루는 등록·제자리 갱신 경로.
//!
//! 자료(`import_file`)와 달리 파일이 원본이므로 해시가 바뀌면 drift가 아니라
//! current revision을 제자리에서 갱신한다.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use sqlx::SqlitePool;

use super::catalog::{create_document, identifier, insert_grant, DocumentDraft};
use super::files::{hash_import, open_scoped_verified, VaultRoot};
use super::scope::ScopeRequest;

/// frontmatter 블록을 읽을 최대 바이트. 넘어가면 frontmatter 없음으로 본다.
const MAX_FRONTMATTER_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WikiFrontmatter {
    pub title: Option<String>,
    pub sources: Vec<String>,
}

/// 파일 앞의 `---` 블록에서 `title`과 `sources`만 읽는다. 나머지 키와 형식이
/// 어긋난 블록은 무시한다(= frontmatter 없음).
pub(crate) fn parse_frontmatter(text: &str) -> WikiFrontmatter {
    let Some(rest) = text.strip_prefix("---\n") else {
        return WikiFrontmatter::default();
    };
    let Some(end) = rest.find("\n---") else {
        return WikiFrontmatter::default();
    };
    let mut front = WikiFrontmatter::default();
    let mut in_sources = false;
    for line in rest[..end].lines() {
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            if in_sources {
                front.sources.push(unquote(item));
            }
            continue;
        }
        in_sources = false;
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key {
            "title" => front.title = Some(unquote(value)).filter(|title| !title.is_empty()),
            "sources" => in_sources = inline_sources(value.trim(), &mut front.sources),
            _ => {}
        }
    }
    front
}

/// `[a, b]` 한 줄 목록이면 채우고, 값이 비었으면 이어지는 `- 항목` 목록을 기다린다.
fn inline_sources(value: &str, sources: &mut Vec<String>) -> bool {
    let Some(inline) = value.strip_prefix('[').and_then(|it| it.strip_suffix(']')) else {
        return value.is_empty();
    };
    sources.extend(
        inline
            .split(',')
            .map(unquote)
            .filter(|item| !item.is_empty()),
    );
    false
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|it| it.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|it| it.strip_suffix('\''))
        })
        .unwrap_or(value)
        .trim()
        .to_owned()
}

/// `relative`가 설정된 위키 폴더 **아래**의 `.md`인가.
///
/// 비교는 경로 컴포넌트 단위다 — 문자열 접두사로 보면 `wikix/a.md`가 `wiki`에
/// 걸린다. `wiki_dir`은 여러 단계여도 된다(`문서/기술-위키/wiki`).
pub(crate) fn is_wiki_path(relative: &str, wiki_dir: &str) -> bool {
    let path = Path::new(relative);
    let mut components = path.components();
    for expected in Path::new(wiki_dir).components() {
        if components.next().map(|actual| actual.as_os_str()) != Some(expected.as_os_str()) {
            return false;
        }
    }
    // 폴더 자신은 문서가 아니다 — 최소한 그 아래 한 단계가 더 있어야 한다.
    if components.next().is_none() {
        return false;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

struct WikiFile {
    sha256: String,
    size: i64,
    front: WikiFrontmatter,
}

fn read_wiki_file(root: &VaultRoot, relative: &str) -> anyhow::Result<WikiFile> {
    let mut file = open_scoped_verified(&root.path, root.device, root.inode, relative)?;
    let mut head = Vec::new();
    (&mut file)
        .take(MAX_FRONTMATTER_BYTES)
        .read_to_end(&mut head)?;
    let front = parse_frontmatter(&String::from_utf8_lossy(&head));
    file.seek(SeekFrom::Start(0))?;
    let (sha256, size) = hash_import(&mut file)?;
    Ok(WikiFile {
        sha256,
        size: size as i64,
        front,
    })
}

/// 위키 폴더 아래 새 파일을 kind `note` 문서로 등록한다. 제목은 frontmatter
/// `title`, 없으면 파일 stem이다.
pub(crate) async fn register_wiki_file(
    pool: &SqlitePool,
    vault_id: &str,
    root: &VaultRoot,
    relative: &str,
    scope: &ScopeRequest,
    now: i64,
) -> anyhow::Result<Vec<String>> {
    let file = read_wiki_file(root, relative)?;
    let title = file.front.title.clone().unwrap_or_else(|| {
        Path::new(relative)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Wiki")
            .to_owned()
    });
    let document = create_document(
        pool,
        &DocumentDraft {
            vault_id: vault_id.into(),
            kind: "note".into(),
            title,
        },
        now,
    )
    .await?;
    let revision_id = identifier("revision")?;
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO vault_revisions (id, document_id, relative_path, sha256, size, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&revision_id).bind(&document.id).bind(relative).bind(&file.sha256).bind(file.size).bind(now).execute(&mut *tx).await?;
    sqlx::query("UPDATE vault_documents SET current_revision = ? WHERE id = ?")
        .bind(&revision_id)
        .bind(&document.id)
        .execute(&mut *tx)
        .await?;
    insert_grant(&mut *tx, &revision_id, scope, now).await?;
    tx.commit().await?;
    let warnings =
        resolve_sources(pool, vault_id, &revision_id, relative, &file.front.sources).await?;
    super::index::index_revision(pool, &revision_id).await?;
    Ok(warnings)
}

/// 이미 등록된 `wiki/` 문서를 파일 기준으로 맞춘다. 해시가 바뀌었으면 current
/// revision을 제자리에서 갱신하고 `wiki_rewritten` 이벤트를 남긴다.
pub(crate) async fn refresh_wiki_file(
    pool: &SqlitePool,
    document_id: &str,
    root: &VaultRoot,
    relative: &str,
    now: i64,
) -> anyhow::Result<Vec<String>> {
    let revision = super::catalog::current_revision(pool, document_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("wiki 문서에 current revision이 없습니다"))?;
    let file = read_wiki_file(root, relative)?;
    if file.sha256 == revision.sha256 {
        activate(pool, document_id).await?;
        super::index::index_revision(pool, &revision.id).await?;
        return Ok(Vec::new());
    }
    if !rewrite_revision(
        pool,
        document_id,
        &revision.id,
        &revision.sha256,
        &file,
        now,
    )
    .await?
    {
        return Ok(vec![format!(
            "{relative}: 같은 해시의 revision이 이미 있어 갱신하지 못했습니다"
        )]);
    }
    let vault_id: String = sqlx::query_scalar("SELECT vault_id FROM vault_documents WHERE id = ?")
        .bind(document_id)
        .fetch_one(pool)
        .await?;
    let warnings =
        resolve_sources(pool, &vault_id, &revision.id, relative, &file.front.sources).await?;
    super::index::index_revision(pool, &revision.id).await?;
    Ok(warnings)
}

/// 제자리 갱신 한 트랜잭션. `UNIQUE(document_id, sha256)` 위반이면 `false`.
async fn rewrite_revision(
    pool: &SqlitePool,
    document_id: &str,
    revision_id: &str,
    previous_hash: &str,
    file: &WikiFile,
    now: i64,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let updated = sqlx::query("UPDATE vault_revisions SET sha256 = ?, size = ? WHERE id = ?")
        .bind(&file.sha256)
        .bind(file.size)
        .bind(revision_id)
        .execute(&mut *tx)
        .await;
    if let Err(error) = updated {
        if is_unique_violation(&error) {
            return Ok(false);
        }
        return Err(error.into());
    }
    sqlx::query("INSERT INTO vault_document_events (id, document_id, event_kind, previous_revision, revision_id, confirmed_hash, previous_hash, created_at) VALUES (?, ?, 'wiki_rewritten', NULL, ?, ?, ?, ?)")
        .bind(identifier("event")?).bind(document_id).bind(revision_id).bind(&file.sha256).bind(previous_hash).bind(now).execute(&mut *tx).await?;
    sqlx::query("UPDATE vault_documents SET state = 'active' WHERE id = ? AND state <> 'archived'")
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database| database.is_unique_violation())
}

async fn activate(pool: &SqlitePool, document_id: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE vault_documents SET state = 'active' WHERE id = ? AND state <> 'archived'")
        .bind(document_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// frontmatter `sources`를 창고 상대 경로로 보고 current revision을 찾아
/// `vault_revision_sources`를 다시 채운다. 못 찾은 항목은 경고로 돌려준다.
pub(crate) async fn resolve_sources(
    pool: &SqlitePool,
    vault_id: &str,
    revision_id: &str,
    relative: &str,
    sources: &[String],
) -> anyhow::Result<Vec<String>> {
    sqlx::query("DELETE FROM vault_revision_sources WHERE revision_id = ?")
        .bind(revision_id)
        .execute(pool)
        .await?;
    let mut warnings = Vec::new();
    for item in sources {
        let Some(path) = normalize_source(item) else {
            warnings.push(format!("{relative}: 출처 {item}을(를) 찾지 못했습니다"));
            continue;
        };
        let found: Option<String> = sqlx::query_scalar("SELECT r.id FROM vault_revisions r JOIN vault_documents d ON d.id = r.document_id WHERE d.vault_id = ? AND r.relative_path = ? AND d.current_revision = r.id")
            .bind(vault_id).bind(&path).fetch_optional(pool).await?;
        let Some(source_revision) = found else {
            warnings.push(format!("{relative}: 출처 {item}을(를) 찾지 못했습니다"));
            continue;
        };
        if source_revision == revision_id {
            continue;
        }
        sqlx::query("INSERT OR IGNORE INTO vault_revision_sources (revision_id, source_revision_id) VALUES (?, ?)")
            .bind(revision_id).bind(&source_revision).execute(pool).await?;
    }
    Ok(warnings)
}

fn normalize_source(item: &str) -> Option<String> {
    let trimmed = item.trim().trim_start_matches("./");
    let path = Path::new(trimmed);
    let normal = path
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)));
    (!trimmed.is_empty() && normal).then(|| trimmed.to_owned())
}
