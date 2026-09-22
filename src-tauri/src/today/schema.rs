//! 금일 할 일 스키마 (설계 0021 §5).
//!
//! `tasks`와 별도 테이블이다 — 저쪽은 워크트리·상태머신을 가진 실행 단위이고,
//! 여기는 사람이 쓰는 계획 레이어다 (설계 0021 DR-1).
//!
//! `carried_from`은 나중에 붙은 컬럼이라 `mod.rs::migrate`가 구버전 DB에 ALTER로 보강한다 —
//! `CREATE TABLE IF NOT EXISTS`는 이미 있는 테이블에 컬럼을 더해 주지 않는다.

/// 다중문 — `sqlx::raw_sql`로만 실행한다.
pub const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS day_items (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  day         TEXT    NOT NULL,
  title       TEXT    NOT NULL,
  note        TEXT,
  status      TEXT    NOT NULL DEFAULT 'open',
  position    INTEGER NOT NULL,
  repo        TEXT,
  task_id     INTEGER,
  source      TEXT    NOT NULL DEFAULT 'manual',
  source_ref  TEXT,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  done_at     INTEGER,
  carried_from TEXT
);

CREATE INDEX IF NOT EXISTS idx_day_items_day ON day_items(day, position);

CREATE UNIQUE INDEX IF NOT EXISTS idx_day_items_task
  ON day_items(task_id) WHERE task_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_day_items_source
  ON day_items(day, source, source_ref) WHERE source_ref IS NOT NULL;

CREATE TABLE IF NOT EXISTS day_closings (
  day        TEXT PRIMARY KEY,
  closed_at  INTEGER NOT NULL,
  done       INTEGER NOT NULL,
  open       INTEGER NOT NULL,
  dropped    INTEGER NOT NULL,
  draft      TEXT NOT NULL
);
"#;
