//! 로컬 지식 그래프 — Gmail·Notion·Obsidian을 하나의 노드·엣지 그래프로 흡수한다.
//!
//! Tauri 비의존(순수) 모듈이다 — `cargo test`로 독립 검증한다.
//! Tauri 경계는 `commands.rs`에만 둔다.
//!
//! **Runner·모바일에 노출하지 않는다.** `runner/`에서 이 모듈을 참조하면
//! `tests::isolation`이 실패한다 (설계 0020 DR-6).
//!
//! 설계 정본: `docs/designs/0020.2026-07-31-local-knowledge-graph-rag-design.md`

pub mod chunk;
pub mod config;
pub mod graph;
pub mod hash;
pub mod link;
pub mod normalize;
pub mod schema;
pub mod search;
pub mod source;
pub mod sync;
pub mod vault;
pub mod wiki;

#[cfg(test)]
mod tests;

use sqlx::SqlitePool;

/// 지식 그래프 스키마를 생성한다. 앱 기동마다 호출되므로 멱등해야 한다.
///
/// **새 컬럼을 `schema::MIGRATION`에 적는 것만으로는 기존 설치에 반영되지 않는다.**
/// `CREATE TABLE IF NOT EXISTS`는 이미 있는 테이블을 그냥 건너뛴다. 아래 보강 ALTER를
/// 같이 넣어야 한다 — 빠뜨리면 신규 설치만 멀쩡하고 기존 사용자는 INSERT가 통째로 깨진다.
pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    // 트리거 본문의 내부 `;` 때문에 수동 분할이 깨진다 → 다중문 실행 API를 쓴다.
    // (`memory/mod.rs:337`이 같은 함정을 주석으로 남겨 두었다.)
    sqlx::raw_sql(schema::MIGRATION).execute(pool).await?;
    // 구버전 DB 보강 — 이미 있으면 에러를 무시한다(멱등).
    // `memory::ensure_memory_columns`와 같은 additive 패턴이다.
    let _ = sqlx::query(
        "ALTER TABLE knowledge_nodes ADD COLUMN embed_enabled INTEGER NOT NULL DEFAULT 1",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query("ALTER TABLE knowledge_nodes ADD COLUMN space_id TEXT")
        .execute(pool)
        .await;
    sqlx::query("CREATE INDEX IF NOT EXISTS knowledge_nodes_space ON knowledge_nodes(space_id)")
        .execute(pool)
        .await?;
    // `doc_title`은 아래 FTS 재구축이 읽어야 하므로 반드시 그보다 먼저 붙인다 —
    // 순서를 뒤집으면 재구축이 `no such column: T.doc_title`로 실패한다.
    let _ = sqlx::query("ALTER TABLE knowledge_chunks ADD COLUMN doc_title TEXT")
        .execute(pool)
        .await;

    rebuild_legacy_fts(pool).await?;
    vault::migrate(pool).await?;
    Ok(())
}

/// 제목을 색인하지 못하는 구버전 FTS를 갈아엎는다.
///
/// 컬럼 추가(위 ALTER)만으로는 절반만 고친 것이다. FTS 가상 테이블과 트리거는
/// `IF NOT EXISTS`라 구버전 정의(본문 1컬럼)가 그대로 남고, 그러면 제목·heading이
/// 색인되지 않는다 — **에러 없이 recall만 떨어지는**, INSERT 실패보다 진단하기 어려운
/// 형태로 고장난다. 검색 질의가 컬럼을 명시하지 않아 구버전에서도 그냥 도는 탓이다.
async fn rebuild_legacy_fts(pool: &SqlitePool) -> anyhow::Result<()> {
    let ddl: Option<(String,)> = sqlx::query_as(
        "SELECT COALESCE(sql, '') FROM sqlite_master WHERE type = 'table' AND name = 'knowledge_fts'",
    )
    .fetch_optional(pool)
    .await?;
    // DDL 원문에 `doc_title`이 없으면 제목을 담을 자리가 없는 정의다.
    // 재구축 후에는 이 조건이 거짓이 되므로 기동마다 반복되지 않는다.
    let is_legacy = ddl.is_some_and(|(sql,)| !sql.contains("doc_title"));
    if !is_legacy {
        return Ok(());
    }

    sqlx::raw_sql(schema::DROP_LEGACY_FTS).execute(pool).await?;
    // 전부 `IF NOT EXISTS`라 방금 지운 FTS·트리거만 다시 생긴다.
    sqlx::raw_sql(schema::MIGRATION).execute(pool).await?;
    // 외부 콘텐츠 인덱스는 트리거로만 채워진다. 이미 쌓인 청크는 되읽지 않으면
    // 영영 색인되지 않으므로, 재구축은 여기서 한 번 명시적으로 돌린다.
    sqlx::query("INSERT INTO knowledge_fts(knowledge_fts) VALUES('rebuild')")
        .execute(pool)
        .await?;
    Ok(())
}
