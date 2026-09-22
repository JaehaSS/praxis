//! Goal Run 스키마 (계획 0036).
//!
//! 다중문이라 `sqlx::raw_sql`로만 실행한다 (`knowledge::schema`와 같은 관례).

pub const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS goal_runs (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  repo           TEXT NOT NULL,
  agent          TEXT NOT NULL,
  instruction    TEXT NOT NULL,
  -- 계약과 예산은 JSON 텍스트로 통째 보관한다. 계약 스키마가 v2로 가도 진행 중인 Run은
  -- 만들어질 때의 v1 계약으로 계속 판정되어야 한다 — 컬럼으로 펼치면 그 성질을 잃는다.
  goal_contract  TEXT NOT NULL,
  budget         TEXT NOT NULL,
  status         TEXT NOT NULL DEFAULT 'running',
  created_at     INTEGER NOT NULL,
  ended_at       INTEGER,
  end_reason     TEXT
);

CREATE INDEX IF NOT EXISTS goal_runs_status ON goal_runs(status);

-- 시도 = Run이 만든 태스크 하나. task_id가 UNIQUE인 이유는 한 태스크가 두 Run에 속할 수
-- 없기 때문이고, 이 제약이 승인/거부 경로에서 "이 태스크의 Run"을 단일 조회로 만든다.
CREATE TABLE IF NOT EXISTS goal_run_attempts (
  run_id     INTEGER NOT NULL REFERENCES goal_runs(id) ON DELETE CASCADE,
  task_id    INTEGER NOT NULL UNIQUE,
  seq        INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  -- 검증 게이트 판정. `verify`는 요청 시 빌드·테스트를 실제로 실행하고 결과를 남기지
  -- 않으므로(`review_ops/verify/execution.rs:17-25`), 시도가 끝난 뒤 크론이 한 번 돌려
  -- 여기 적는다. 매 틱 재실행하면 60초마다 빌드가 돈다.
  --
  -- 두 컬럼인 이유 — 세 상태를 구분해야 한다:
  --   evaluated_at NULL              → 아직 평가하지 않았다 (다음 틱에 평가한다)
  --   evaluated_at 있고 ready NULL   → 평가했으나 검증 커맨드가 없다 → 사람에게 넘긴다
  --   evaluated_at 있고 ready 0/1    → 판정 완료
  -- 한 컬럼으로 접으면 "커맨드 없음"이 "실패"로 읽혀 무한 재시도가 된다.
  gate_ready        INTEGER,
  gate_evaluated_at INTEGER,
  PRIMARY KEY (run_id, seq)
);

CREATE INDEX IF NOT EXISTS goal_run_attempts_task ON goal_run_attempts(task_id);
"#;
