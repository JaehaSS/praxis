//! 동기화 오케스트레이션 — 커넥터와 저장소를 잇는다.
//!
//! **커서는 모든 문서를 커밋한 뒤에만 전진한다.** 먼저 올리면 중간에 죽었을 때 그 구간이
//! 영원히 스킵된다. 재처리는 `content_hash` 스킵이 흡수하므로 거의 공짜지만,
//! 누락은 사용자가 "왜 이 노트가 안 찾아지지"로 겪는다 — 재처리는 싸고 누락은 비싸다.

use sqlx::{Row, SqlitePool};

use super::graph::{self, Document};
use super::link::{parse_wikilinks, TargetIndex};

/// 커넥터가 돌려주는 한 번의 변경 묶음.
#[derive(Debug)]
pub struct SourceChanges {
    pub upserts: Vec<Document>,
    /// 소스가 삭제를 직접 알려주는 경우(Gmail history 등). 전량 스캔형은 비운다.
    pub deletions: Vec<String>,
    pub next_cursor: Option<String>,
    /// true면 `upserts`가 소스의 **전량**이다 → 저장된 것과 diff해 삭제를 도출한다.
    /// Obsidian은 true, Gmail·Notion(증분)은 false가 된다.
    pub full_scan: bool,
    /// 같은 소스를 한 번 더 불러야 남은 것이 오는가. 백필처럼 페이지로 쪼개진
    /// 소스가 쓴다.
    ///
    /// **커서가 비었는지로 대신 판정하면 안 된다.** Gmail 증분은 항상 다음 `historyId`를
    /// 남기므로 커서가 영원히 비지 않아, 반복 루프가 멈추지 않는다.
    pub has_more: bool,
}

/// 커넥터가 돌려주는 future. **`async fn`을 쓰지 못한다** — `sync_source`가
/// `&dyn Source`로 받는데 native async fn in trait은 object-safe하지 않기 때문이다.
/// 반환 타입을 직접 박싱해 우회한다 (설계 0020 Phase 4 / 플랜 0028 DR-A).
pub type BoxChanges<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<SourceChanges>> + Send + 'a>>;

/// `Send + Sync`가 필요한 이유: Tauri 커맨드는 `Send` future를 요구하는데,
/// `sync_source`가 `&dyn Source`를 await 경계 너머로 들고 간다.
pub trait Source: Send + Sync {
    fn id(&self) -> &str;
    /// Obsidian은 파일시스템이라 즉시 끝나지만, Gmail·Notion은 네트워크를 탄다.
    /// 그래서 계약 자체가 async다 — 동기 구현은 `Box::pin(async move { … })`로 감싼다.
    fn changes<'a>(&'a self, cursor: Option<&'a str>) -> BoxChanges<'a>;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub indexed: usize,
    pub skipped: usize,
    pub deleted: usize,
    pub edges: usize,
    /// 마지막 배치가 "아직 남았다"고 말했는가. 진행률 표시와 반복 종료에 쓴다.
    pub has_more: bool,
}

/// 남은 것이 없을 때까지 반복한다 — **백필이 중단돼도 이어지는 지점**이 여기다.
///
/// 재개 로직을 따로 짜지 않았다는 점이 핵심이다. `sync_source`가 이미 *커밋한 뒤에만*
/// 커서를 전진시키므로, 페이지 단위로 쪼개는 것만으로 중단 안전성이 따라온다.
///
/// `max_batches`는 한 번의 호출을 끊는 예산이다. 3만 통 백필을 한 커맨드로 붙잡고 있으면
/// UI가 진행 상황을 보여줄 수 없고, 사용자는 멈춘 것과 구분하지 못한다.
pub async fn sync_until_done(
    pool: &SqlitePool,
    source: &dyn Source,
    now: i64,
    max_batches: usize,
) -> anyhow::Result<SyncReport> {
    let mut total = SyncReport::default();
    for _ in 0..max_batches {
        let batch = sync_source(pool, source, now).await?;
        total.indexed += batch.indexed;
        total.skipped += batch.skipped;
        total.deleted += batch.deleted;
        total.edges += batch.edges;
        total.has_more = batch.has_more;
        if !batch.has_more {
            break;
        }
    }
    Ok(total)
}

pub async fn sync_source(
    pool: &SqlitePool,
    source: &dyn Source,
    now: i64,
) -> anyhow::Result<SyncReport> {
    let cursor = load_cursor(pool, source.id()).await?;
    let changes = source.changes(cursor.as_deref()).await?;
    let mut report = SyncReport {
        has_more: changes.has_more,
        ..SyncReport::default()
    };

    for doc in &changes.upserts {
        match graph::upsert_document(pool, doc, now).await? {
            graph::UpsertOutcome::Indexed => report.indexed += 1,
            graph::UpsertOutcome::Skipped => report.skipped += 1,
        }
    }

    let mut deletions = changes.deletions.clone();
    if changes.full_scan {
        let stored = stored_ids(pool, source.id()).await?;
        let present: Vec<String> = changes
            .upserts
            .iter()
            .map(|d| d.external_id.clone())
            .collect();
        deletions.extend(graph::deleted_ids(&stored, &present));
    }
    for id in &deletions {
        graph::delete_document(pool, source.id(), id).await?;
        report.deleted += 1;
    }

    // 링크 해소는 upsert가 **전부 끝난 뒤** 한 번에 한다. vault 순회 순서가 보장되지
    // 않으므로, 대상이 아직 없다고 엣지를 버리면 알파벳 순서에 따라 그래프가 달라진다.
    report.edges = rebuild_links(pool, source.id(), &changes.upserts).await?;

    // 여기까지 왔을 때만 커서를 올린다.
    save_cursor(pool, source.id(), changes.next_cursor.as_deref(), now).await?;
    Ok(report)
}

/// 이번 묶음에 포함된 문서의 아웃링크를 다시 만든다.
pub(crate) async fn rebuild_links(
    pool: &SqlitePool,
    source: &str,
    docs: &[Document],
) -> anyhow::Result<usize> {
    if docs.is_empty() {
        return Ok(0);
    }
    let rows = sqlx::query("SELECT id, external_id FROM knowledge_nodes WHERE source = ?")
        .bind(source)
        .fetch_all(pool)
        .await?;
    let nodes: Vec<(i64, String)> = rows
        .iter()
        .filter_map(|r| Some((r.try_get("id").ok()?, r.try_get("external_id").ok()?)))
        .collect();
    let index = TargetIndex::build(&nodes);
    let id_of: std::collections::HashMap<&str, i64> = nodes
        .iter()
        .map(|(id, ext)| (ext.as_str(), *id))
        .collect();

    let mut written = 0usize;
    let mut tx = pool.begin().await?;
    for doc in docs {
        let Some(src_id) = id_of.get(doc.external_id.as_str()).copied() else {
            continue;
        };
        sqlx::query("DELETE FROM knowledge_edges WHERE src_id = ? AND rel = 'links_to'")
            .bind(src_id)
            .execute(&mut *tx)
            .await?;
        for target in parse_wikilinks(&doc.body) {
            let Some(dst_id) = index.resolve(&target) else {
                continue; // 아직 없는 노트로의 링크 — 유령 노드를 만들지 않는다
            };
            if dst_id == src_id {
                continue; // 자기 참조는 그래프에 의미가 없다
            }
            let done = sqlx::query(
                "INSERT OR IGNORE INTO knowledge_edges (src_id, dst_id, rel) \
                 VALUES (?, ?, 'links_to')",
            )
            .bind(src_id)
            .bind(dst_id)
            .execute(&mut *tx)
            .await?;
            written += done.rows_affected() as usize;
        }
    }
    tx.commit().await?;
    Ok(written)
}

pub async fn load_cursor(pool: &SqlitePool, source: &str) -> anyhow::Result<Option<String>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT cursor FROM knowledge_sources WHERE id = ?")
            .bind(source)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|r| r.0))
}

async fn save_cursor(
    pool: &SqlitePool,
    source: &str,
    cursor: Option<&str>,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO knowledge_sources (id, status, cursor, last_sync, last_error) \
         VALUES (?, 'connected', ?, ?, NULL) \
         ON CONFLICT(id) DO UPDATE SET \
           status = 'connected', cursor = excluded.cursor, \
           last_sync = excluded.last_sync, last_error = NULL",
    )
    .bind(source)
    .bind(cursor)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

async fn stored_ids(pool: &SqlitePool, source: &str) -> anyhow::Result<Vec<String>> {
    let rows = sqlx::query("SELECT external_id FROM knowledge_nodes WHERE source = ?")
        .bind(source)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .filter_map(|r| r.try_get("external_id").ok())
        .collect())
}
