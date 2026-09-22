//! 스키마 생성·멱등성·FTS 설정 검증.

use super::{raw_pool, test_pool};

/// `doc_title`·`embed_enabled` 도입 이전에 **실제로 배포됐던** 정의.
/// 사용자 DB에서 확인한 그대로다 — 결함이 재현되지 않으면 이 상수부터 의심할 것.
/// 여기 없는 테이블(sources·edges)은 이 결함과 무관해 `migrate`가 만들도록 둔다.
const LEGACY_SCHEMA: &str = r#"
CREATE TABLE knowledge_nodes (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  source        TEXT NOT NULL,
  external_id   TEXT NOT NULL,
  kind          TEXT NOT NULL,
  title         TEXT NOT NULL,
  url           TEXT,
  content_hash  TEXT,
  updated_at    INTEGER NOT NULL,
  synced_at     INTEGER NOT NULL,
  UNIQUE(source, external_id)
);
CREATE TABLE knowledge_chunks (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  node_id     INTEGER NOT NULL REFERENCES knowledge_nodes(id) ON DELETE CASCADE,
  ord         INTEGER NOT NULL,
  heading     TEXT,
  content     TEXT NOT NULL,
  embedding   BLOB,
  embed_model TEXT,
  UNIQUE(node_id, ord)
);
CREATE VIRTUAL TABLE knowledge_fts
  USING fts5(content, content='knowledge_chunks', content_rowid='id',
             tokenize='trigram');
CREATE TRIGGER knowledge_chunks_ai AFTER INSERT ON knowledge_chunks BEGIN
  INSERT INTO knowledge_fts(rowid, content) VALUES (new.id, new.content);
END;
CREATE TRIGGER knowledge_chunks_ad AFTER DELETE ON knowledge_chunks BEGIN
  INSERT INTO knowledge_fts(knowledge_fts, rowid, content) VALUES('delete', old.id, old.content);
END;
CREATE TRIGGER knowledge_chunks_au AFTER UPDATE ON knowledge_chunks BEGIN
  INSERT INTO knowledge_fts(knowledge_fts, rowid, content) VALUES('delete', old.id, old.content);
  INSERT INTO knowledge_fts(rowid, content) VALUES (new.id, new.content);
END;
"#;

async fn insert_node(pool: &sqlx::SqlitePool, external_id: &str, title: &str) {
    sqlx::query(
        "INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) \
         VALUES ('obsidian', ?, 'document', ?, 1, 1)",
    )
    .bind(external_id)
    .bind(title)
    .execute(pool)
    .await
    .unwrap();
}

async fn fts_hits(pool: &sqlx::SqlitePool, query: &str) -> i64 {
    let (hits,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM knowledge_fts WHERE knowledge_fts MATCH ?")
            .bind(query)
            .fetch_one(pool)
            .await
            .unwrap();
    hits
}

#[tokio::test]
async fn migrate_is_idempotent() {
    let pool = test_pool().await;
    // 두 번째 호출이 에러 없이 통과해야 한다 — 앱은 매 기동마다 migrate를 부른다.
    crate::knowledge::migrate(&pool).await.unwrap();

    let (tables,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' \
         AND name IN ('knowledge_sources','knowledge_nodes','knowledge_chunks','knowledge_edges')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tables, 4);
}

#[tokio::test]
async fn fts_uses_the_trigram_tokenizer() {
    let pool = test_pool().await;
    let (sql,): (String,) =
        sqlx::query_as("SELECT sql FROM sqlite_master WHERE name = 'knowledge_fts'")
            .fetch_one(&pool)
            .await
            .unwrap();
    // trigram: 한국어 substring 매칭(설계 0020 DR-3).
    assert!(sql.contains("trigram"), "trigram 토크나이저가 아니다: {sql}");
    // `detail`을 낮추면 trigram 매칭 자체가 불가능해진다 —
    // trigram은 3-gram 시퀀스를 찾는 phrase 질의이고, phrase는 detail=full에서만 된다.
    assert!(
        !sql.contains("detail"),
        "detail을 낮추면 trigram 질의가 런타임에 거부된다: {sql}"
    );
}

/// 위 제약을 실제 질의로 고정한다. 스키마 문자열 검사만으로는
/// "왜 detail을 못 낮추는지"가 다음 사람에게 전달되지 않는다.
#[tokio::test]
async fn trigram_match_actually_runs() {
    let pool = test_pool().await;
    sqlx::query(
        "INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) \
         VALUES ('obsidian', 'n.md', 'document', '제목', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, content) \
         VALUES ((SELECT id FROM knowledge_nodes), 0, '지식그래프를 설계한다')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 어절 경계를 가로지르는 substring — 기본 토크나이저면 0건이다.
    let (hits,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM knowledge_fts WHERE knowledge_fts MATCH '지식그래프'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(hits, 1);
}

/// FTS 외부 콘텐츠 인덱스는 트리거가 없으면 조용히 비어 있는다.
/// 검색이 0건을 내는데 데이터는 멀쩡해 보이는, 진단이 어려운 형태로 고장난다.
#[tokio::test]
async fn fts_triggers_keep_the_index_in_sync() {
    let pool = test_pool().await;
    sqlx::query(
        "INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) \
         VALUES ('obsidian', 'n.md', 'document', '제목', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, content) \
         VALUES ((SELECT id FROM knowledge_nodes), 0, '고유단어 크세논')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let (hits,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM knowledge_fts WHERE knowledge_fts MATCH '크세논'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(hits, 1, "INSERT 트리거가 FTS에 반영하지 않았다");

    sqlx::query("DELETE FROM knowledge_chunks")
        .execute(&pool)
        .await
        .unwrap();
    let (ghosts,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM knowledge_fts WHERE knowledge_fts MATCH '크세논'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(ghosts, 0, "DELETE 트리거가 없어 FTS에 유령 행이 남았다");
}

/// 노드를 지우면 청크·엣지가 함께 사라져야 한다. 소스 연결 해제 시 원문이 남으면
/// "끊었는데 아직 검색된다"가 되어 신뢰를 잃는다(설계 0020 §10).
#[tokio::test]
async fn deleting_a_node_cascades_to_chunks() {
    let pool = test_pool().await;
    sqlx::query(
        "INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) \
         VALUES ('obsidian', 'n.md', 'document', '제목', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, content) \
         VALUES ((SELECT id FROM knowledge_nodes), 0, '본문')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("DELETE FROM knowledge_nodes")
        .execute(&pool)
        .await
        .unwrap();
    let (chunks,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge_chunks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(chunks, 0, "CASCADE가 걸리지 않았다 — PRAGMA foreign_keys 확인");
}

/// 구버전 DB 업그레이드 경로. 위 테스트들은 전부 빈 DB에서 시작하므로 **신규 설치만**
/// 검증한다 — 그 틈으로 `doc_title` 보강 누락이 통째로 새어나가, 기존 설치의 동기화가
/// `table knowledge_chunks has no column named doc_title`로 멈췄다.
#[tokio::test]
async fn migrate_upgrades_a_legacy_database() {
    let pool = raw_pool().await;
    sqlx::raw_sql(LEGACY_SCHEMA).execute(&pool).await.unwrap();

    crate::knowledge::migrate(&pool).await.unwrap();

    // 1) 컬럼이 붙어야 한다 — 동기화가 실제로 쓰는 INSERT 형태 그대로 확인한다.
    insert_node(&pool, "n.md", "영상요약프롬프트").await;
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, doc_title, heading, content) \
         VALUES ((SELECT id FROM knowledge_nodes), 0, '영상요약프롬프트', '개요', '본문')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // `embed_enabled`도 같은 커밋에서 들어왔다. 한쪽만 보강한 것이 이번 결함이었다.
    sqlx::query("SELECT embed_enabled FROM knowledge_nodes")
        .fetch_one(&pool)
        .await
        .unwrap();

    // 2) 제목이 색인돼야 한다. FTS가 구버전(본문 1컬럼)으로 남으면 여기서 0건이 된다 —
    //    에러가 아니라 recall 저하로만 드러나는, 컬럼 누락보다 조용한 고장이다.
    assert_eq!(
        fts_hits(&pool, "영상요약프롬프트").await,
        1,
        "제목이 색인되지 않았다 — FTS가 구버전 정의로 남아 있다"
    );
}

/// 재구축은 인덱스를 비우고 다시 채운다. `rebuild`를 빠뜨리면 업그레이드 순간
/// **기존 청크가 통째로 검색에서 사라진다** — 재동기화 전까지 조용히.
#[tokio::test]
async fn upgrading_preserves_already_indexed_chunks() {
    let pool = raw_pool().await;
    sqlx::raw_sql(LEGACY_SCHEMA).execute(&pool).await.unwrap();
    insert_node(&pool, "n.md", "제목").await;
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, content) \
         VALUES ((SELECT id FROM knowledge_nodes), 0, '고유단어 크세논')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        fts_hits(&pool, "크세논").await,
        1,
        "구버전 인덱스 전제가 깨졌다"
    );

    crate::knowledge::migrate(&pool).await.unwrap();

    assert_eq!(
        fts_hits(&pool, "크세논").await,
        1,
        "FTS 재구축이 기존 청크를 되읽지 않았다"
    );
    // 다만 기존 행의 `doc_title`은 NULL이다 — 제목은 재동기화가 채운다.
    // 재구축이 복원하는 것은 어디까지나 이미 컬럼에 있던 값이다.
}

/// 재구축은 **한 번만** 일어나야 한다. 매 기동 돌면 청크 11만 건 규모에서 시동이
/// 눈에 띄게 느려진다.
///
/// "인덱스가 살아 있는가"로는 이걸 못 잡는다 — 재구축이 반복돼도 결과는 같기 때문이다.
/// 그래서 인덱스를 일부러 비워 두고, 재구축이 돌았다면 다시 채워졌을 자리가
/// 비어 있는지를 본다.
#[tokio::test]
async fn upgrading_the_fts_runs_only_once() {
    let pool = raw_pool().await;
    sqlx::raw_sql(LEGACY_SCHEMA).execute(&pool).await.unwrap();
    crate::knowledge::migrate(&pool).await.unwrap();
    insert_node(&pool, "n.md", "제목").await;
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, doc_title, content) \
         VALUES ((SELECT id FROM knowledge_nodes), 0, '제목', '고유단어 크세논')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO knowledge_fts(knowledge_fts) VALUES('delete-all')")
        .execute(&pool)
        .await
        .unwrap();

    crate::knowledge::migrate(&pool).await.unwrap();

    assert_eq!(
        fts_hits(&pool, "크세논").await,
        0,
        "이미 최신인 FTS를 다시 재구축했다 — 기동마다 전체 재색인이 돈다"
    );
}
