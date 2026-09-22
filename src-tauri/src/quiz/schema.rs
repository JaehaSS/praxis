//! 대기 퀴즈 스키마 (설계 0044 "데이터 모델").
//!
//! `quiz_items.chunk_id`가 `knowledge_chunks`를 참조하므로 **`knowledge::migrate`가 선행해야
//! 한다** — 순서를 뒤집으면 참조 대상이 없어 CREATE부터 실패한다.

/// 다중문 — `sqlx::raw_sql`로만 실행한다 (`knowledge/schema.rs:6`과 같은 이유).
pub const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS quiz_items (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  -- 'domain' | 'vocab' | 'coding' | 'trivia'
  kind        TEXT NOT NULL,
  question    TEXT NOT NULL,
  -- JSON 배열. NULL이면 단답형.
  choices     TEXT,
  answer      TEXT NOT NULL,
  explanation TEXT,
  -- domain일 때만 채운다. 출처가 사라진 도메인 문제는 검증할 수 없으므로 함께 지운다.
  chunk_id    INTEGER REFERENCES knowledge_chunks(id) ON DELETE CASCADE,
  -- 검수 시점의 근거 본문 복제본. 청크가 나중에 바뀌어도 "무엇을 보고 승인했는가"는 남아야 한다.
  source_excerpt TEXT,
  -- 'pending' | 'approved' | 'retired'
  status      TEXT NOT NULL,
  created_at  INTEGER NOT NULL,
  reviewed_at INTEGER
);

-- 풀다 만 상태를 담는다. 대기가 끝나도 지우지 않는다 — 다음 대기에서 이어 풀기 위함(설계 DR-3).
CREATE TABLE IF NOT EXISTS quiz_attempts (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id     INTEGER NOT NULL REFERENCES quiz_items(id) ON DELETE CASCADE,
  -- 'open' | 'answered'
  state       TEXT NOT NULL,
  picked      TEXT,
  correct     INTEGER,
  opened_at   INTEGER NOT NULL,
  answered_at INTEGER
);

-- 출제 쿼리가 status로 먼저 거르므로(pending·retired 제외) 첫 컬럼이 status다.
CREATE INDEX IF NOT EXISTS quiz_items_serve ON quiz_items(status, kind);
CREATE INDEX IF NOT EXISTS quiz_attempts_open ON quiz_attempts(state, item_id);
"#;
