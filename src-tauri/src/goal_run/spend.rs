//! 예산 사용량을 `convo_events` 원장에서 파생한다 (계획 0036 DR-3).
//!
//! 새 카운터 테이블을 두지 않는 이유는 #235와 같다 — 원장이 이미 있고, 파생 가능한 값을
//! 중복 저장하면 두 벌이 어긋난다. `tool_cost::analyze`가 같은 패턴의 선례다.

use serde_json::Value;
use sqlx::SqlitePool;

use super::Spent;

/// 시도 하나가 쓴 양.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spend {
    pub tokens: i64,
    pub cost_usd: f64,
    /// 벤더가 보고한 에이전트 내부 턴 수. **예산이 아니라 표시용**이다 (Phase 2 탐색 Q2).
    pub vendor_turns: i64,
}

impl Spend {
    fn add(&mut self, other: Spend) {
        self.tokens += other.tokens;
        self.cost_usd += other.cost_usd;
        self.vendor_turns += other.vendor_turns;
    }
}

/// 원장 행(JSON 문자열)에서 소비량을 합산한다.
///
/// `ConvoEvent::Result`(턴 종료)만 센다. 한 태스크가 후속 입력으로 여러 턴을 돌 수 있으므로
/// **합산**이 맞다 — 최댓값을 쓰면 대화가 길어질수록 실제보다 적게 잡힌다.
///
/// `ConvoEvent`로 역직렬화하지 않고 `Value`로 느슨하게 읽는다. `convo_events`에는
/// `{"kind":"user"}` 같은 비-`ConvoEvent` 행이 섞여 있다 (#235).
pub fn from_events(events: &[String]) -> Spend {
    let mut total = Spend::default();
    for raw in events {
        let Ok(v) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        if v.get("kind").and_then(Value::as_str) != Some("result") {
            continue;
        }
        total.add(Spend {
            tokens: field_i64(&v, "tokens_in") + field_i64(&v, "tokens_out"),
            // 벤더 비대칭 — codex는 비용을 주지 않아 이 필드가 없거나 0이다.
            // 없는 값을 추정으로 채우지 않는다(#235).
            cost_usd: v.get("cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
            vendor_turns: field_i64(&v, "num_turns"),
        });
    }
    total
}

fn field_i64(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// Run 전체의 사용량. 시도들의 원장을 모아 합산하고 경과 시간을 붙인다.
///
/// 크래시로 턴이 끝나지 못한 시도는 `Result`가 없어 토큰·비용이 0으로 계상된다.
/// 그 경우를 막는 것은 `max_attempts`다 — 시도 수는 원장이 아니라 우리 테이블에서 세므로
/// 에이전트가 무엇을 남겼든 항상 증가한다.
pub async fn collect(
    pool: &SqlitePool,
    run_id: i64,
    created_at: i64,
    now: i64,
) -> anyhow::Result<Spent> {
    let task_ids = super::attempt_task_ids(pool, run_id).await?;
    let mut total = Spend::default();
    for task_id in &task_ids {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT event FROM convo_events WHERE task_id = ? ORDER BY id")
                .bind(task_id)
                .fetch_all(pool)
                .await?;
        let events: Vec<String> = rows.into_iter().map(|r| r.0).collect();
        total.add(from_events(&events));
    }
    Ok(Spent {
        attempts: task_ids.len() as i64,
        tokens: total.tokens,
        cost_usd: total.cost_usd,
        elapsed_secs: (now - created_at).max(0),
    })
}
