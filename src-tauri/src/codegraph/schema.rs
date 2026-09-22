//! 코드 구조 그래프 스키마 (계획 0037 DR-2).
//!
//! `knowledge_*`와 **별도 테이블**이다 — 저쪽은 외부 소스 동기화(`cursor`/`synced_at`)와
//! FTS·임베딩 모델이고, 코드는 워크트리 로컬이며 파일 해시로 통째 무효화된다.
//! 섞으면 코드 재인덱싱이 `knowledge_fts` 트리거를 매번 때려 무관한 문서 검색을 느리게 만든다.
//! `knowledge/schema.rs:1-4`가 memories와 갈라서며 남긴 논리를 한 번 더 적용한 것이다.

/// 다중문 — `sqlx::raw_sql`로만 실행한다.
pub const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS code_files (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  worktree     TEXT NOT NULL,
  rel_path     TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  lang         TEXT,
  indexed_at   INTEGER NOT NULL,
  -- LSP 서버가 없거나 응답하지 않은 사유. NULL이면 정상 인덱싱됨.
  -- 빈 결과와 미지원을 구분하지 못하면 "데이터는 있는데 검색이 0건"이 된다.
  skip_reason  TEXT,
  UNIQUE(worktree, rel_path)
);

CREATE TABLE IF NOT EXISTS code_nodes (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  file_id   INTEGER NOT NULL REFERENCES code_files(id) ON DELETE CASCADE,
  name      TEXT NOT NULL,
  kind      INTEGER NOT NULL,          -- LSP SymbolKind 원본
  container TEXT,                      -- 부모 심볼 이름 (평탄화 시 보존)
  sel_line  INTEGER NOT NULL,          -- LSP 원본 0-based. UI 경계에서만 +1 한다
  sel_char  INTEGER NOT NULL,
  end_line  INTEGER NOT NULL,
  UNIQUE(file_id, name, sel_line, sel_char)
);

CREATE TABLE IF NOT EXISTS code_edges (
  src_id INTEGER NOT NULL REFERENCES code_nodes(id) ON DELETE CASCADE,
  dst_id INTEGER NOT NULL REFERENCES code_nodes(id) ON DELETE CASCADE,
  rel    TEXT NOT NULL,                -- 'references' | 'contains'
  PRIMARY KEY (src_id, dst_id, rel)
);

-- impact_of는 dst에서 src로 거슬러 오른다 — 역참조가 이 인덱스를 탄다.
CREATE INDEX IF NOT EXISTS code_edges_dst ON code_edges(dst_id);
CREATE INDEX IF NOT EXISTS code_nodes_file ON code_nodes(file_id);
CREATE INDEX IF NOT EXISTS code_nodes_name ON code_nodes(name);
-- 워크트리가 지워질 때 그 행들을 찾아 지운다(purge_worktree). 인덱스가 없으면
-- 정리가 전체 스캔이 되고, 정리를 미루면 고아 데이터가 조용히 쌓인다.
CREATE INDEX IF NOT EXISTS code_files_worktree ON code_files(worktree);

-- 세대형 스냅샷. legacy code_*는 롤백 경계로 그대로 둔다(설계 0055 DR-4).
CREATE TABLE IF NOT EXISTS code_graph_runs (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  worktree           TEXT NOT NULL,
  state              TEXT NOT NULL,
  source_fingerprint TEXT NOT NULL,
  started_at         INTEGER NOT NULL,
  finished_at        INTEGER,
  files_seen         INTEGER NOT NULL DEFAULT 0,
  files_skipped      INTEGER NOT NULL DEFAULT 0,
  symbols            INTEGER NOT NULL DEFAULT 0,
  edges              INTEGER NOT NULL DEFAULT 0,
  failure_reason     TEXT
);

CREATE TABLE IF NOT EXISTS code_graph_active (
  worktree TEXT PRIMARY KEY,
  run_id   INTEGER NOT NULL REFERENCES code_graph_runs(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS code_graph_files (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id       INTEGER NOT NULL REFERENCES code_graph_runs(id) ON DELETE CASCADE,
  rel_path     TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  lang         TEXT NOT NULL,          -- LSP languageId (설계 0065 DR-4). 서버 키가 아니다
  -- NULL이 아니면: 심볼조차 만들지 못했다 (설계 0065 DR-3b)
  skip_reason  TEXT,
  -- NULL이 아니면: 심볼은 있으나 엣지를 만들지 않았다 (사유)
  edge_state   TEXT,
  UNIQUE(run_id, rel_path)
);

CREATE TABLE IF NOT EXISTS code_graph_nodes (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id          INTEGER NOT NULL REFERENCES code_graph_runs(id) ON DELETE CASCADE,
  file_id         INTEGER NOT NULL REFERENCES code_graph_files(id) ON DELETE CASCADE,
  name            TEXT NOT NULL,
  kind            INTEGER NOT NULL,
  container       TEXT,
  sel_start_line  INTEGER NOT NULL,
  sel_start_char  INTEGER NOT NULL,
  sel_end_line    INTEGER NOT NULL,
  sel_end_char    INTEGER NOT NULL,
  body_start_line INTEGER NOT NULL,
  body_start_char INTEGER NOT NULL,
  body_end_line   INTEGER NOT NULL,
  body_end_char   INTEGER NOT NULL,
  UNIQUE(run_id, file_id, name, sel_start_line, sel_start_char)
);

CREATE TABLE IF NOT EXISTS code_graph_edges (
  run_id INTEGER NOT NULL REFERENCES code_graph_runs(id) ON DELETE CASCADE,
  src_id INTEGER NOT NULL REFERENCES code_graph_nodes(id) ON DELETE CASCADE,
  dst_id INTEGER NOT NULL REFERENCES code_graph_nodes(id) ON DELETE CASCADE,
  rel    TEXT NOT NULL,
  PRIMARY KEY(run_id, src_id, dst_id, rel)
);

CREATE INDEX IF NOT EXISTS code_graph_runs_worktree ON code_graph_runs(worktree, id DESC);
CREATE INDEX IF NOT EXISTS code_graph_files_run_path ON code_graph_files(run_id, rel_path);
CREATE INDEX IF NOT EXISTS code_graph_nodes_file ON code_graph_nodes(file_id);
CREATE INDEX IF NOT EXISTS code_graph_edges_dst ON code_graph_edges(run_id, dst_id);
"#;
