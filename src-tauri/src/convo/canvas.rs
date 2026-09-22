//! 작업 캔버스 — `Plan` 이벤트를 Mermaid 흐름도로 압축한다.
//!
//! 사람이 보는 그림이 아니라 **에이전트가 읽는 압축 표현**이다(계획 0033 DR-5). 다음 세션이
//! Capsule로 이 텍스트를 물려받아 "어디까지 했나"를 산문보다 적은 토큰으로 파악한다.
//! 노드 ID 형식(`NNN-N<seq>`)과 `%%{...}%%` 메타 블록은 TencentDB Agent Memory 관례를 따른다.
//!
//! 원천이 LLM 추출이 아니라 에이전트가 이미 발행하는 TodoWrite라는 것이 핵심이다(DR-6) —
//! 추출 LLM은 비용·지연·오류를 셋 다 더한다.

use crate::convo::PlanItem;

/// Mermaid 라벨 한 칸의 문자 상한. Rust 코드포인트 기준이며 프론트에서 다시 자르지 않는다
/// (JS `String.length`는 UTF-16이라 한국어·이모지에서 어긋난다 — 교훈 #189).
const LABEL_LIMIT: usize = 100;

/// 벤더 상태 문자열 → 캔버스 표기. 모르는 값은 `todo`로 접는다(관측은 원문을 이미 보존한다).
fn status_mark(status: &str) -> &'static str {
    match status {
        "completed" => "done",
        "in_progress" => "doing",
        _ => "todo",
    }
}

/// Mermaid 라벨 안에서 구조를 깨거나 managed block을 닫을 수 있는 문자를 무해화한다.
///
/// `"`는 라벨을 조기 종료시키고, `[`·`]`는 노드 경계를, `<`·`>`는 `<br/>`와 managed 마커
/// (`<!-- ... -->`)를 위조한다. 에이전트가 쓴 문자열이 그대로 들어오는 자리라 신뢰할 수 없다.
fn escape_label(value: &str) -> String {
    value
        .replace('"', "'")
        .replace('[', "(")
        .replace(']', ")")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .chars()
        .take(LABEL_LIMIT)
        .collect()
}

/// 계획 항목들을 Mermaid 흐름도로. 빈 목록은 빈 문자열 — 호출부가 섹션 자체를 생략한다.
pub fn render(items: &[PlanItem], seq: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = format!("%%{{ \"seq\": {seq} }}%%\nflowchart TD\n");
    for (i, item) in items.iter().enumerate() {
        out.push_str(&format!(
            "  {seq:03}-N{n}[\"status: {status}<br/>summary: {summary}\"]\n",
            n = i + 1,
            status = status_mark(&item.status),
            summary = escape_label(&item.content),
        ));
    }
    for i in 1..items.len() {
        // 간선은 `-->` 그대로 — 라벨 밖은 HTML 주석 컨텍스트가 아니라 이스케이프가 불필요하고,
        // `--&gt;`로 쓰면 Mermaid가 파싱하지 못한다. capsule managed 마커는 각각 완결된
        // 주석(`<!-- ... -->`)이라 본문의 `-->`가 블록을 닫지 않는다.
        out.push_str(&format!("  {seq:03}-N{i} --> {seq:03}-N{}\n", i + 1));
    }
    out
}

/// 이벤트 목록에서 **마지막** 메인 스레드 Plan 스냅샷만 캔버스로 만든다.
///
/// TodoWrite는 갱신 때마다 전체 목록을 다시 보내므로 누적하면 같은 항목이 중복된다.
/// 서브 에이전트(`parent_id`)의 계획은 그 에이전트의 것이지 이 작업의 것이 아니라 제외한다.
/// `seq`는 스냅샷이 몇 번째인지 — 캔버스가 갱신됐음을 노드 ID로 드러낸다.
pub fn latest_from_events(events: &[String]) -> String {
    let mut latest: Option<Vec<PlanItem>> = None;
    let mut seq = 0usize;
    for line in events {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("kind").and_then(|k| k.as_str()) != Some("plan") {
            continue;
        }
        if v.get("parent_id").is_some_and(|p| !p.is_null()) {
            continue;
        }
        let Some(items) = v.get("items").and_then(|i| i.as_array()) else {
            continue;
        };
        let parsed: Vec<PlanItem> = items
            .iter()
            .filter_map(|it| {
                Some(PlanItem {
                    content: it.get("content")?.as_str()?.to_string(),
                    status: it
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("pending")
                        .to_string(),
                })
            })
            .collect();
        if !parsed.is_empty() {
            seq += 1;
            latest = Some(parsed);
        }
    }
    latest.map(|items| render(&items, seq)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(content: &str, status: &str) -> PlanItem {
        PlanItem {
            content: content.into(),
            status: status.into(),
        }
    }

    #[test]
    fn renders_nodes_with_status_and_edges() {
        let mmd = render(
            &[
                item("스키마 확인", "completed"),
                item("주입 수정", "in_progress"),
            ],
            1,
        );
        assert!(mmd.starts_with("%%{ \"seq\": 1 }%%\nflowchart TD\n"));
        assert!(mmd.contains("001-N1[\"status: done<br/>summary: 스키마 확인\"]"));
        assert!(mmd.contains("001-N2[\"status: doing<br/>summary: 주입 수정\"]"));
        // 간선은 Mermaid가 실제로 파싱하는 형태여야 한다.
        assert!(mmd.contains("001-N1 --> 001-N2"));
        assert!(!mmd.contains("--&gt;"));
    }

    /// 스냅샷이 999를 넘으면 노드 ID가 4자리가 된다 — 프론트 파서가 이를 견뎌야 한다.
    #[test]
    fn node_id_widens_past_three_digits() {
        let mmd = render(&[item("x", "pending"), item("y", "pending")], 1234);
        assert!(mmd.contains("1234-N1"));
        assert!(mmd.contains("1234-N1 --> 1234-N2"));
    }

    #[test]
    fn unknown_status_folds_to_todo() {
        assert!(render(&[item("x", "저기요")], 1).contains("status: todo"));
        assert!(render(&[item("x", "pending")], 1).contains("status: todo"));
    }

    /// 라벨은 에이전트가 쓴 문자열이 그대로 들어오는 자리다 — 구조를 깨면 안 된다.
    #[test]
    fn labels_cannot_break_mermaid_or_managed_block() {
        let hostile = format!("따옴표 \" 와 <br/> 와 ] 와 {}", crate::capsule::CAP_END);
        let mmd = render(&[item(&hostile, "pending")], 1);

        // 노드를 여는 `["`와 닫는 `"]`가 각각 한 번씩만 — 본문이 경계를 위조하지 못한다.
        assert_eq!(mmd.matches("[\"").count(), 1);
        assert_eq!(mmd.matches("\"]").count(), 1);
        assert!(!mmd.contains(crate::capsule::CAP_END));
        assert!(!mmd.contains("<br/> 와"));
    }

    #[test]
    fn label_is_truncated_by_codepoints() {
        let mmd = render(&[item(&"가".repeat(300), "pending")], 1);
        let label = mmd.split("summary: ").nth(1).unwrap();
        assert_eq!(
            label.chars().take_while(|c| *c == '가').count(),
            LABEL_LIMIT
        );
    }

    #[test]
    fn empty_plan_renders_nothing() {
        assert!(render(&[], 1).is_empty());
        assert!(latest_from_events(&[]).is_empty());
    }

    /// TodoWrite는 전체 목록을 매번 다시 보낸다 — 누적하면 같은 항목이 중복된다.
    #[test]
    fn only_the_last_snapshot_survives() {
        let events = vec![
            r#"{"kind":"plan","items":[{"content":"첫 계획","status":"pending"}]}"#.to_string(),
            r#"{"kind":"plan","items":[{"content":"둘째 계획","status":"completed"}]}"#.to_string(),
        ];
        let mmd = latest_from_events(&events);
        assert!(mmd.contains("둘째 계획"));
        assert!(!mmd.contains("첫 계획"));
        // 두 번째 스냅샷이므로 seq=2 → 노드 ID가 002-*.
        assert!(mmd.contains("002-N1"));
    }

    /// 서브 에이전트의 계획은 그 에이전트의 것이지 이 작업의 것이 아니다.
    #[test]
    fn subagent_plan_is_ignored() {
        let events = vec![
            r#"{"kind":"plan","items":[{"content":"내 계획","status":"pending"}]}"#.to_string(),
            r#"{"kind":"plan","parent_id":"task1","items":[{"content":"하위 계획","status":"pending"}]}"#
                .to_string(),
        ];
        let mmd = latest_from_events(&events);
        assert!(mmd.contains("내 계획"));
        assert!(!mmd.contains("하위 계획"));
    }

    /// convo_events에는 ConvoEvent가 아닌 행과 깨진 줄이 섞여 있다.
    #[test]
    fn foreign_rows_are_skipped() {
        let events = vec![
            r#"{"kind":"user","text":"안녕"}"#.to_string(),
            "깨진 JSON {{{".to_string(),
            r#"{"kind":"plan","items":[]}"#.to_string(),
            r#"{"kind":"plan","items":[{"content":"살아남는 계획","status":"pending"}]}"#
                .to_string(),
        ];
        let mmd = latest_from_events(&events);
        assert!(mmd.contains("살아남는 계획"));
        // 빈 items는 스냅샷으로 세지 않으므로 seq는 1이다.
        assert!(mmd.contains("001-N1"));
    }
}
