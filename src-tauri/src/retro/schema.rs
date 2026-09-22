//! 주간 회고 다이제스트 스키마 (설계 0054 §6.3).
//!
//! `week_start`가 UNIQUE인 이유는 한 주에 다이제스트가 하나이기 때문이다. 같은 주를 두 번
//! 쓰면 어느 쪽이 맞는지 알 수 없어, 재생성은 덮어쓰기가 아니라 **거부**다.
//!
//! `facts`를 함께 보관하는 이유가 이 설계의 핵심이다(DR-7). 서술은 LLM이 쓰지만 수치는
//! Rust가 계산해 프롬프트에 넣는다 — 그 원본을 남기지 않으면 "출처"가 아무것도 증명하지
//! 못하고, 서술이 숫자를 틀렸는지 대조할 방법도 사라진다.

pub const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS retro_digests (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  -- 스케줄의 tz_offset_secs 기준 월요일 00:00을 UTC epoch로 환산한 값.
  week_start   INTEGER NOT NULL UNIQUE,
  -- LLM이 쓴 서술. 문단 3~5개.
  body         TEXT NOT NULL,
  -- 프롬프트에 넣은 RetroFacts JSON 원본 (DR-7).
  facts        TEXT NOT NULL,
  agent        TEXT,
  model        TEXT,
  generated_at INTEGER NOT NULL
);
"#;
