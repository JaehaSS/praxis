use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use sqlx::{Row, SqlitePool};

use crate::knowledge::vault::{
    migrate, register_vault, scan_vault, search_browse, Scope, ScopeRequest,
};

use super::super::wiki::parse_frontmatter;

fn directory(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let path = crate::testtmp::dir().join(format!(
        "vault-wiki-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn private_scope() -> ScopeRequest {
    ScopeRequest {
        scope: Scope::PrivateData,
    }
}

fn write(root: &Path, relative: &str, body: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// (문서 id, kind, 제목, revision id, sha256, 상태)
async fn document(
    pool: &SqlitePool,
    relative: &str,
) -> Option<(String, String, String, String, String, String)> {
    let row = sqlx::query("SELECT d.id, d.kind, d.title, r.id AS revision_id, r.sha256, d.state FROM vault_documents d JOIN vault_revisions r ON r.id = d.current_revision WHERE r.relative_path = ?")
        .bind(relative)
        .fetch_optional(pool)
        .await
        .unwrap()?;
    Some((
        row.get("id"),
        row.get("kind"),
        row.get("title"),
        row.get("revision_id"),
        row.get("sha256"),
        row.get("state"),
    ))
}

async fn count(pool: &SqlitePool, sql: &str, binding: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(binding)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[cfg(unix)]
#[tokio::test]
async fn scan_registers_wiki_markdown_as_note_with_frontmatter_title() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("register");
    write(
        &root,
        "wiki/redis.md",
        "---\ntitle: \"Redis 메모\"\n---\n## 핵심 요약\n캐시 전략 정리\n",
    );
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let result = scan_vault(&pool, &vault.id, private_scope(), 2)
        .await
        .unwrap();
    assert_eq!(result.indexed, 1);
    let (_, kind, title, _, _, state) = document(&pool, "wiki/redis.md").await.unwrap();
    assert_eq!(kind, "note");
    assert_eq!(title, "Redis 메모");
    assert_eq!(state, "active");
    let hits = search_browse(&pool, "캐시 전략", 0).await.unwrap();
    assert_eq!(hits.hits.len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn scan_follows_the_configured_wiki_folder_instead_of_the_default() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("configured");
    crate::knowledge::vault::settings::store(&pool, "문서/기술-위키/wiki", "wiki-organizer", "위키-시작.md")
        .await
        .unwrap();
    write(&root, "문서/기술-위키/wiki/redis.md", "본문\n");
    // 기본값이던 루트 `wiki/`는 더 이상 위키가 아니다 — 자료로 들어간다.
    write(&root, "wiki/legacy.md", "옛 본문\n");
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    scan_vault(&pool, &vault.id, private_scope(), 2)
        .await
        .unwrap();
    let (_, kind, title, _, _, _) = document(&pool, "문서/기술-위키/wiki/redis.md")
        .await
        .unwrap();
    assert_eq!(kind, "note");
    assert_eq!(title, "redis");
    let (_, legacy_kind, _, _, _, _) = document(&pool, "wiki/legacy.md").await.unwrap();
    assert_ne!(legacy_kind, "note");
}

#[cfg(unix)]
#[tokio::test]
async fn wiki_file_without_frontmatter_takes_the_file_stem_as_title() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("stem");
    write(&root, "wiki/a.md", "본문만 있다\n");
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    scan_vault(&pool, &vault.id, private_scope(), 2)
        .await
        .unwrap();
    let (_, kind, title, _, _, _) = document(&pool, "wiki/a.md").await.unwrap();
    assert_eq!(kind, "note");
    assert_eq!(title, "a");
}

#[cfg(unix)]
#[tokio::test]
async fn rewritten_wiki_file_updates_the_current_revision_in_place() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("rewrite");
    write(&root, "wiki/redis.md", "---\ntitle: Redis\n---\n첫 본문\n");
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    scan_vault(&pool, &vault.id, private_scope(), 2)
        .await
        .unwrap();
    let (document_id, _, _, revision_id, first_hash, _) =
        document(&pool, "wiki/redis.md").await.unwrap();
    let grants = count(
        &pool,
        "SELECT COUNT(*) FROM vault_grants WHERE revision_id = ?",
        &revision_id,
    )
    .await;
    write(
        &root,
        "wiki/redis.md",
        "---\ntitle: Redis\n---\n고친 본문\n",
    );
    let result = scan_vault(&pool, &vault.id, private_scope(), 3)
        .await
        .unwrap();
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    let (_, _, _, same_revision, second_hash, state) =
        document(&pool, "wiki/redis.md").await.unwrap();
    assert_eq!(same_revision, revision_id);
    assert_ne!(second_hash, first_hash);
    assert_eq!(state, "active");
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM vault_grants WHERE revision_id = ?",
            &revision_id
        )
        .await,
        grants
    );
    let event = sqlx::query("SELECT event_kind, previous_hash, confirmed_hash, revision_id FROM vault_document_events WHERE document_id = ?")
        .bind(&document_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(event.get::<String, _>("event_kind"), "wiki_rewritten");
    assert_eq!(event.get::<String, _>("previous_hash"), first_hash);
    assert_eq!(event.get::<String, _>("confirmed_hash"), second_hash);
    assert_eq!(event.get::<String, _>("revision_id"), revision_id);
    assert_eq!(
        search_browse(&pool, "고친 본문", 0)
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    assert!(search_browse(&pool, "첫 본문", 0)
        .await
        .unwrap()
        .hits
        .is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn wiki_sources_resolve_known_paths_and_warn_on_the_rest() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("sources");
    write(&root, "notes/x.txt", "자료 본문");
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    scan_vault(&pool, &vault.id, private_scope(), 2)
        .await
        .unwrap();
    write(
        &root,
        "wiki/redis.md",
        "---\ntitle: Redis\nsources: [./notes/x.txt, notes/none.txt]\n---\n본문\n",
    );
    let result = scan_vault(&pool, &vault.id, private_scope(), 3)
        .await
        .unwrap();
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert!(result.warnings[0].contains("notes/none.txt"));
    let (_, _, _, revision_id, _, _) = document(&pool, "wiki/redis.md").await.unwrap();
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM vault_revision_sources WHERE revision_id = ?",
            &revision_id
        )
        .await,
        1
    );
}

#[cfg(unix)]
#[tokio::test]
async fn scan_excludes_agent_configuration_at_the_vault_root() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("exclusions");
    write(&root, ".claude/skills/x.md", "스킬");
    write(&root, "CLAUDE.md", "루트 지침");
    write(&root, "AGENTS.md", "루트 지침");
    write(&root, "GEMINI.md", "루트 지침");
    write(&root, "sub/CLAUDE.md", "하위 지침");
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let result = scan_vault(&pool, &vault.id, private_scope(), 2)
        .await
        .unwrap();
    assert_eq!(result.indexed, 1);
    assert!(document(&pool, ".claude/skills/x.md").await.is_none());
    assert!(document(&pool, "CLAUDE.md").await.is_none());
    assert!(document(&pool, "sub/CLAUDE.md").await.is_some());
}

#[cfg(unix)]
#[tokio::test]
async fn frontmatter_scope_does_not_change_the_requested_grant() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("scope");
    write(
        &root,
        "wiki/redis.md",
        "---\ntitle: Redis\nscope: common\n---\n본문\n",
    );
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    scan_vault(&pool, &vault.id, private_scope(), 2)
        .await
        .unwrap();
    let (_, _, _, revision_id, _, _) = document(&pool, "wiki/redis.md").await.unwrap();
    let scopes: Vec<String> =
        sqlx::query_scalar("SELECT scope FROM vault_grants WHERE revision_id = ?")
            .bind(&revision_id)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(scopes, vec!["private-data".to_string()]);
}

#[cfg(unix)]
#[tokio::test]
async fn deleted_wiki_file_is_marked_missing() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("missing");
    write(&root, "wiki/redis.md", "---\ntitle: Redis\n---\n본문\n");
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    scan_vault(&pool, &vault.id, private_scope(), 2)
        .await
        .unwrap();
    std::fs::remove_file(root.join("wiki/redis.md")).unwrap();
    scan_vault(&pool, &vault.id, private_scope(), 3)
        .await
        .unwrap();
    let (_, _, _, _, _, state) = document(&pool, "wiki/redis.md").await.unwrap();
    assert_eq!(state, "missing");
}

#[tokio::test]
async fn migration_adds_the_previous_hash_column_once() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    migrate(&pool).await.unwrap();
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('vault_document_events')")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        columns
            .iter()
            .filter(|column| column.as_str() == "previous_hash")
            .count(),
        1
    );
}

#[test]
fn frontmatter_parses_title_and_source_lists() {
    let quoted = parse_frontmatter(
        "---\ntitle: \"Redis 메모\"\nsources:\n  - a.md\n  - 'b.md'\n---\n본문\n",
    );
    assert_eq!(quoted.title.as_deref(), Some("Redis 메모"));
    assert_eq!(quoted.sources, vec!["a.md".to_string(), "b.md".to_string()]);
    let inline =
        parse_frontmatter("---\ntitle: Redis\nsources: [a.md, b.md]\ngenerator: x\n---\n본문");
    assert_eq!(inline.title.as_deref(), Some("Redis"));
    assert_eq!(inline.sources, vec!["a.md".to_string(), "b.md".to_string()]);
    let unclosed = parse_frontmatter("---\ntitle: Redis\n본문만 이어진다\n");
    assert_eq!(unclosed.title, None);
    assert!(unclosed.sources.is_empty());
    let none = parse_frontmatter("# 제목\n본문\n");
    assert_eq!(none.title, None);
}

#[test]
fn wiki_paths_need_the_wiki_folder_and_a_markdown_extension() {
    use super::super::wiki::is_wiki_path;
    assert!(is_wiki_path("wiki/redis.md", "wiki"));
    assert!(is_wiki_path("wiki/team/redis.MD", "wiki"));
    assert!(!is_wiki_path("wiki/redis.txt", "wiki"));
    assert!(!is_wiki_path("notes/redis.md", "wiki"));
    assert!(!is_wiki_path("wiki.md", "wiki"));
    // 폴더 자신은 문서가 아니다.
    assert!(!is_wiki_path("wiki", "wiki"));
}

#[test]
fn a_configured_wiki_folder_can_be_nested_and_is_matched_by_component() {
    use super::super::wiki::is_wiki_path;
    let dir = "문서/기술-위키/wiki";
    assert!(is_wiki_path("문서/기술-위키/wiki/redis.md", dir));
    assert!(is_wiki_path("문서/기술-위키/wiki/팀/redis.md", dir));
    assert!(!is_wiki_path("wiki/redis.md", dir));
    assert!(!is_wiki_path("문서/기술-위키/redis.md", dir));
    // 문자열 접두사로 보면 걸리는 것들 — 컴포넌트 비교라 걸리지 않는다.
    assert!(!is_wiki_path("wikix/redis.md", "wiki"));
    assert!(!is_wiki_path("문서/기술-위키/wikix/redis.md", dir));
}
