//! 회고 생성 프롬프트 조립 (설계 0054 DR-4·DR-5).
//!
//! 퀴즈(`quiz/generate.rs`)와 같은 골격이다 — 헤드리스 에이전트에게 줄 instruction을 만들고,
//! 결과는 DB가 아니라 JSON 파일로 받는다.
//!
//! **다른 점이 하나 있다: 여기서는 숫자를 미리 계산해 프롬프트에 박아 넣는다.** 에이전트는
//! 그 수치를 인용만 하고 새로 세지 않는다. 회고에서 틀릴 수 있는 것은 거의 전부 숫자이고,
//! 한 번 틀린 회고는 다시 읽히지 않는다(DR-4).

use super::RetroFacts;

/// 앱 데이터 디렉터리 밑의 inbox 위치.
///
/// 워크트리가 아닌 이유는 퀴즈와 같다 — 워크트리는 작업이 끝나면 사라질 수 있어 거기 쓴
/// 결과를 거둘 수 없다.
pub const INBOX_SUBDIR: &str = "retro/inbox";

/// 헤드리스 에이전트에게 줄 instruction. `inbox_dir`는 **절대 경로**여야 한다.
///
/// 대상 주에 작업이 하나도 없으면 `None`이다 — 쓸 것이 없는데 문장을 만들라고 시키면
/// 에이전트는 없는 이야기를 지어낸다.
pub fn build_instruction(facts: &RetroFacts, inbox_dir: &str) -> Option<String> {
    if facts.tasks_total == 0 {
        return None;
    }
    let facts_json = serde_json::to_string_pretty(facts).ok()?;

    let mut prompt = String::new();
    prompt.push_str(
        "너는 개발자 한 사람의 주간 작업 기록을 회고로 정리한다.\n\n\
         아래 수치는 이미 확정된 값이다. **이 값을 그대로 인용하고, 직접 세거나 계산하지 마라.**\n\
         주어지지 않은 수치는 언급하지 마라 — 모르는 것은 쓰지 않는 편이 낫다.\n\n",
    );
    prompt.push_str("```json\n");
    prompt.push_str(&facts_json);
    prompt.push_str("\n```\n\n");

    prompt.push_str(
        "필드의 뜻:\n\
         - `tasks_total`/`tasks_done`/`tasks_discarded` — 그 주에 만들어진 작업 수와 결말\n\
         - `discard_rate_pct` — 폐기율. `discard_rate_prev_pct`는 직전 주 값(없으면 null)\n\
         - `followup_pct` — 후속 입력이 있었던 작업의 비율. **횟수가 아니라 발생 여부다**\n\
         - `proposals_pending`/`proposals_applied` — 자기개선 제안의 검토 대기·채택 누적 건수\n\
         - `top_role` — 그 주에 가장 많이 쓴 역할과 그 완료율\n\n",
    );

    prompt.push_str(
        "3~5개 문단으로 한국어 평서문을 써라. 규칙:\n\
         - 사실을 먼저 쓰고 해석은 그다음에 붙인다\n\
         - 칭찬하거나 격려하지 마라. 관찰만 남긴다\n\
         - 수치가 나빠졌으면 나빠졌다고 쓴다\n\
         - 원인을 단정하지 마라. 데이터가 말하지 않는 인과는 추측이라고 밝힌다\n\n",
    );

    prompt.push_str(&format!(
        "결과를 `{inbox_dir}/<유닉스초>.json`에 아래 형태로 저장하라. 그 디렉터리가 없으면 \
         만들어라. 다른 곳에 쓰거나 DB를 직접 건드리지 마라.\n\n\
         {{\"week_start\":{week_start},\"body\":\"...\"}}\n",
        inbox_dir = inbox_dir,
        week_start = facts.week_start,
    ));

    Some(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> RetroFacts {
        RetroFacts {
            week_start: 1_700_000_000,
            tasks_total: 23,
            tasks_done: 17,
            tasks_discarded: 4,
            discard_rate_pct: 17.4,
            discard_rate_prev_pct: Some(24.0),
            followup_pct: 58.0,
            proposals_pending: 762,
            proposals_applied: 0,
            top_role: None,
        }
    }

    #[test]
    fn embeds_confirmed_numbers_and_forbids_recounting() {
        let prompt = build_instruction(&facts(), "/tmp/praxis/retro/inbox").unwrap();
        assert!(prompt.contains("\"tasks_total\": 23"));
        assert!(prompt.contains("직접 세거나 계산하지 마라"));
        assert!(prompt.contains("/tmp/praxis/retro/inbox/<유닉스초>.json"));
        assert!(prompt.contains("\"week_start\":1700000000"));
    }

    /// 빈 주에 문장을 시키면 없는 이야기가 나온다.
    #[test]
    fn refuses_empty_week() {
        let empty = RetroFacts {
            tasks_total: 0,
            ..facts()
        };
        assert!(build_instruction(&empty, "/tmp/x").is_none());
    }
}
