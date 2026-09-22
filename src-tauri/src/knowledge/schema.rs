//! 지식 그래프 스키마 (설계 0020 §7).
//!
//! `memories`와 **별도 테이블**이다 — 저쪽은 `/v1/memories`로 Runner에 노출돼 있고
//! evidence·승인·Must-Apply 거버넌스가 걸려 있다. 외부 문서를 섞으면 둘 다 깨진다 (DR-1).

/// 다중문 — `sqlx::raw_sql`로만 실행한다 (트리거 본문의 `;` 때문).
pub const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS knowledge_sources (
  id          TEXT PRIMARY KEY,
  status      TEXT NOT NULL DEFAULT 'disconnected',
  cursor      TEXT,
  config      TEXT,
  last_sync   INTEGER,
  last_error  TEXT
);

CREATE TABLE IF NOT EXISTS knowledge_nodes (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  source        TEXT NOT NULL,
  space_id      TEXT,
  external_id   TEXT NOT NULL,
  kind          TEXT NOT NULL,
  title         TEXT NOT NULL,
  url           TEXT,
  content_hash  TEXT,
  updated_at    INTEGER NOT NULL,
  synced_at     INTEGER NOT NULL,
  -- 0이면 임베딩 대상에서 뺀다. 색인(FTS)은 그대로 되므로 **검색은 계속 된다** —
  -- 어휘 검색만 되고 의미 검색이 안 될 뿐이다.
  -- 실측 vault는 청크의 98%가 Claude Code 세션 로그였고, 그걸 임베딩하면 6시간이 걸린다.
  -- 그 정도 비용을 들일 가치가 있는 문서와 아닌 문서를 사람이 가르게 하는 손잡이다.
  embed_enabled INTEGER NOT NULL DEFAULT 1,
  UNIQUE(source, external_id)
);

CREATE TABLE IF NOT EXISTS knowledge_chunks (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  node_id     INTEGER NOT NULL REFERENCES knowledge_nodes(id) ON DELETE CASCADE,
  ord         INTEGER NOT NULL,
  -- 노트 제목을 청크에 복제한다(비정규화). 제목은 가장 강한 검색 신호인데
  -- 외부 콘텐츠 FTS는 다른 테이블의 컬럼을 색인할 수 없어서, 여기 두어야 색인된다.
  doc_title   TEXT,
  heading     TEXT,
  content     TEXT NOT NULL,
  embedding   BLOB,
  embed_model TEXT,
  UNIQUE(node_id, ord)
);

CREATE TABLE IF NOT EXISTS knowledge_edges (
  src_id   INTEGER NOT NULL REFERENCES knowledge_nodes(id) ON DELETE CASCADE,
  dst_id   INTEGER NOT NULL REFERENCES knowledge_nodes(id) ON DELETE CASCADE,
  rel      TEXT NOT NULL,
  weight   REAL NOT NULL DEFAULT 1.0,
  derived  INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (src_id, dst_id, rel)
);

CREATE INDEX IF NOT EXISTS knowledge_edges_dst ON knowledge_edges(dst_id);
CREATE INDEX IF NOT EXISTS knowledge_nodes_source ON knowledge_nodes(source);
CREATE INDEX IF NOT EXISTS knowledge_chunks_node ON knowledge_chunks(node_id);

-- trigram: 한국어를 어절이 아니라 substring으로 매칭한다. 기본 토크나이저면
--   "지식그래프를"이 "지식그래프"에 걸리지 않는다 (DR-3).
--
-- `detail=none`을 쓰려 했으나 **불가능하다.** trigram 매칭은 연속된 3-gram 시퀀스를
-- 찾는 것이라 본질적으로 phrase 질의이고, `detail`이 `full`이 아니면 SQLite가
-- `fts5: phrase queries are not supported (detail!=full)`로 거부한다.
-- 즉 trigram과 detail 축소는 양립하지 않는다 — 인덱스 크기(본문의 3~4배)를 대가로 받는다.
--
-- 주의: trigram은 3자 미만 질의를 매칭하지 못한다 — 짧은 질의는 제목 매칭으로 폴백한다.
-- 제목·heading·본문을 함께 색인한다. 본문만 넣으면 "영상 요약 프롬프트"라는 제목의
-- 노트를 같은 문구로 검색해도 못 찾는다 — 실측에서 확인한 누락이다.
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts
  USING fts5(doc_title, heading, content, content='knowledge_chunks',
             content_rowid='id', tokenize='trigram');

-- 외부 콘텐츠 FTS는 트리거로만 동기화된다. 빠뜨리면 인덱스가 조용히 비어
-- "데이터는 있는데 검색이 0건"이라는 진단하기 어려운 형태로 고장난다.
CREATE TRIGGER IF NOT EXISTS knowledge_chunks_ai AFTER INSERT ON knowledge_chunks BEGIN
  INSERT INTO knowledge_fts(rowid, doc_title, heading, content)
    VALUES (new.id, new.doc_title, new.heading, new.content);
END;
CREATE TRIGGER IF NOT EXISTS knowledge_chunks_ad AFTER DELETE ON knowledge_chunks BEGIN
  INSERT INTO knowledge_fts(knowledge_fts, rowid, doc_title, heading, content)
    VALUES('delete', old.id, old.doc_title, old.heading, old.content);
END;
CREATE TRIGGER IF NOT EXISTS knowledge_chunks_au AFTER UPDATE ON knowledge_chunks BEGIN
  INSERT INTO knowledge_fts(knowledge_fts, rowid, doc_title, heading, content)
    VALUES('delete', old.id, old.doc_title, old.heading, old.content);
  INSERT INTO knowledge_fts(rowid, doc_title, heading, content)
    VALUES (new.id, new.doc_title, new.heading, new.content);
END;
"#;

/// 구버전 FTS(본문 1컬럼)와 그 트리거를 걷어낸다.
///
/// `MIGRATION`은 전부 `IF NOT EXISTS`라 **이미 있는 가상 테이블·트리거를 갱신하지 않는다.**
/// 컬럼을 늘리려면 지우고 다시 만드는 수밖에 없다. 테이블 정의는 `MIGRATION`에만 두고
/// 여기서는 제거만 한다 — 두 벌로 나누면 한쪽만 고치는 사고가 난다.
pub const DROP_LEGACY_FTS: &str = r#"
DROP TRIGGER IF EXISTS knowledge_chunks_ai;
DROP TRIGGER IF EXISTS knowledge_chunks_ad;
DROP TRIGGER IF EXISTS knowledge_chunks_au;
DROP TABLE IF EXISTS knowledge_fts;
"#;
