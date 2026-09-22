//! 툴 결과가 메인 스레드 컨텍스트를 얼마나 먹었는지 귀속 분석.
//!
//! 원장은 `convo_events`이고 여기엔 파생 계산만 있다 — 별도 저장소를 두면 두 원장이
//! 어긋날 때 어느 쪽이 정본인지 문제가 생긴다(계획 0033 DR-3).
//!
//! **추정하지 않는다.** 한 관측 구간에 메인 스레드 툴 결과가 정확히 하나일 때만 그 구간의
//! 컨텍스트 증가분을 귀속시키고, 모호하면 `unattributed_tokens`로 남긴다. 크기 비례로 나눠
//! 넣으면 그럴듯한 숫자가 나오지만 근거가 없다 — 계약에 없는 값은 비워 두는 편이 정직하다(#118).
//!
//! `ConvoEvent`에 `Deserialize`를 붙이지 않고 `Value`로 읽는 이유: `convo_events`에는
//! `ConvoEvent`가 아닌 행도 섞여 있어(`{"kind":"user"}`, `user_expanded`) 열거형 역직렬화가
//! 그 행들에서 실패한다(DR-4).

use std::collections::HashMap;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolCostRow {
    pub tool: String,
    /// 메인 스레드 호출 수.
    pub calls: i64,
    /// 원문 문자 수 합계. `result_chars`가 없는 이벤트는 여기 더하지 않는다.
    pub chars: i64,
    /// 크기를 모르는 호출 수 — `chars`의 신뢰도 표시용.
    pub calls_unknown_size: i64,
    /// 모호하지 않은 구간에서만 누적된 실측 토큰.
    pub attributed_tokens: i64,
    pub attributed_calls: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolCostReport {
    pub rows: Vec<ToolCostRow>,
    pub total_chars: i64,
    /// 관측된 컨텍스트 점유 최댓값 — 세션이 얼마나 찼었는지.
    pub peak_context_tokens: i64,
    /// 어느 툴에도 귀속시킬 수 없었던 증가분 합계. 0이 아닌 것이 정상이다.
    pub unattributed_tokens: i64,
}

/// 한 줄이 `kind`를 가진 JSON 객체면 `(kind, value)`.
fn kind_of(line: &str) -> Option<(String, serde_json::Value)> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let k = v.get("kind")?.as_str()?.to_string();
    Some((k, v))
}

/// `parent_id`가 없으면 메인 스레드. 서브 에이전트(Task)는 별도 컨텍스트라 귀속 대상이 아니다.
fn is_main_thread(v: &serde_json::Value) -> bool {
    v.get("parent_id").is_none_or(serde_json::Value::is_null)
}

fn row_for<'a>(acc: &'a mut HashMap<String, ToolCostRow>, tool: &str) -> &'a mut ToolCostRow {
    acc.entry(tool.to_string()).or_insert_with(|| ToolCostRow {
        tool: tool.to_string(),
        calls: 0,
        chars: 0,
        calls_unknown_size: 0,
        attributed_tokens: 0,
        attributed_calls: 0,
    })
}

pub fn analyze(events: &[String]) -> ToolCostReport {
    // tool_use_id → 툴 이름.
    let mut names: HashMap<String, String> = HashMap::new();
    // id 없는 벤더용 폴백 — 직전 메인 스레드 tool_use의 이름.
    let mut last_tool_name: Option<String> = None;
    let mut acc: HashMap<String, ToolCostRow> = HashMap::new();
    // 직전 context_usage 관측 이후 등장한 메인 스레드 결과들의 툴 이름.
    let mut pending: Vec<String> = Vec::new();
    let mut prev_ctx: Option<i64> = None;
    let mut peak = 0i64;
    let mut unattributed = 0i64;

    for line in events {
        let Some((kind, v)) = kind_of(line) else {
            continue;
        };
        match kind.as_str() {
            "tool_use" => {
                let Some(name) = v.get("name").and_then(|x| x.as_str()) else {
                    continue;
                };
                if let Some(id) = v.get("tool_id").and_then(|x| x.as_str()) {
                    names.insert(id.to_string(), name.to_string());
                }
                if is_main_thread(&v) {
                    last_tool_name = Some(name.to_string());
                }
            }
            "tool_result" => {
                if !is_main_thread(&v) {
                    continue;
                }
                // codex는 tool_use_id를 주지 않아 id 매칭이 항상 실패한다. 그대로 두면 모든
                // codex 결과가 한 버킷에 뭉쳐 툴별 분해가 무의미해지므로, 직전 tool_use 이름을
                // 폴백으로 쓴다. codex는 started/completed가 인접한 순차 실행이라 안전하고,
                // claude는 id가 항상 있어 이 경로를 타지 않는다.
                let tool = v
                    .get("tool_use_id")
                    .and_then(|x| x.as_str())
                    .and_then(|id| names.get(id).cloned())
                    .or_else(|| last_tool_name.clone())
                    .unwrap_or_else(|| "(unknown)".to_string());
                let row = row_for(&mut acc, &tool);
                row.calls += 1;
                match v.get("result_chars").and_then(|x| x.as_i64()) {
                    Some(c) => row.chars += c,
                    // 미상은 0이 아니다 — 합계에 넣지 않고 따로 센다.
                    None => row.calls_unknown_size += 1,
                }
                pending.push(tool);
            }
            "context_usage" => {
                if v.get("valid").and_then(|x| x.as_bool()) == Some(false) {
                    prev_ctx = None;
                    pending.clear();
                    continue;
                }
                let Some(ctx) = v.get("context_tokens").and_then(|x| x.as_i64()) else {
                    continue;
                };
                peak = peak.max(ctx);
                if let Some(prev) = prev_ctx {
                    // 음수 델타는 compact/캐시 경계다. 툴이 되돌린 것이 아니므로 계상하지 않는다.
                    let delta = ctx - prev;
                    if delta > 0 {
                        match pending.len() {
                            1 => {
                                let row = row_for(&mut acc, &pending[0]);
                                row.attributed_tokens += delta;
                                row.attributed_calls += 1;
                            }
                            0 => {
                                // 어시스턴트 텍스트·사용자 입력이 늘린 몫. 툴 비용이 아니다.
                            }
                            _ => unattributed += delta,
                        }
                    }
                }
                prev_ctx = Some(ctx);
                pending.clear();
            }
            "context_cleared" => {
                prev_ctx = None;
                pending.clear();
            }
            _ => {}
        }
    }

    let mut rows: Vec<ToolCostRow> = acc.into_values().collect();
    // 실측 귀속이 먼저, 그다음 원문 크기 — 사용자가 "무엇을 줄일까"를 판단하는 순서.
    rows.sort_by(|a, b| {
        b.attributed_tokens
            .cmp(&a.attributed_tokens)
            .then(b.chars.cmp(&a.chars))
            .then(a.tool.cmp(&b.tool))
    });
    let total_chars = rows.iter().map(|r| r.chars).sum();
    ToolCostReport {
        rows,
        total_chars,
        peak_context_tokens: peak,
        unattributed_tokens: unattributed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(json: &str) -> String {
        json.to_string()
    }

    fn row<'a>(r: &'a ToolCostReport, tool: &str) -> &'a ToolCostRow {
        r.rows
            .iter()
            .find(|x| x.tool == tool)
            .unwrap_or_else(|| panic!("{tool} 행이 없다: {:?}", r.rows))
    }

    /// 단일 툴 결과 뒤의 컨텍스트 증가분은 그 툴에 온전히 귀속된다.
    #[test]
    fn single_result_between_observations_is_attributed() {
        let evs = vec![
            ev(r#"{"kind":"context_usage","context_tokens":10000}"#),
            ev(r#"{"kind":"tool_use","name":"Read","tool_id":"t1","summary":"a.rs"}"#),
            ev(
                r#"{"kind":"tool_result","tool_use_id":"t1","result_chars":8000,"summary":"","is_error":false}"#,
            ),
            ev(r#"{"kind":"context_usage","context_tokens":12500}"#),
        ];
        let r = analyze(&evs);
        assert_eq!(r.unattributed_tokens, 0);
        let read = row(&r, "Read");
        assert_eq!(read.attributed_tokens, 2500);
        assert_eq!(read.attributed_calls, 1);
        assert_eq!(read.chars, 8000);
        assert_eq!(read.calls, 1);
        assert_eq!(r.total_chars, 8000);
    }

    /// 한 구간에 결과가 둘 이상이면 크기 비례로 나누지 않고 미귀속으로 남긴다.
    #[test]
    fn ambiguous_interval_is_not_split() {
        let evs = vec![
            ev(r#"{"kind":"context_usage","context_tokens":10000}"#),
            ev(r#"{"kind":"tool_use","name":"Read","tool_id":"t1","summary":""}"#),
            ev(
                r#"{"kind":"tool_result","tool_use_id":"t1","result_chars":100,"summary":"","is_error":false}"#,
            ),
            ev(r#"{"kind":"tool_use","name":"Bash","tool_id":"t2","summary":""}"#),
            ev(
                r#"{"kind":"tool_result","tool_use_id":"t2","result_chars":900,"summary":"","is_error":false}"#,
            ),
            ev(r#"{"kind":"context_usage","context_tokens":13000}"#),
        ];
        let r = analyze(&evs);
        assert_eq!(r.unattributed_tokens, 3000);
        assert!(r.rows.iter().all(|x| x.attributed_tokens == 0));
        // 귀속만 못 할 뿐 크기·호출 수는 그대로 집계된다.
        assert_eq!(row(&r, "Bash").chars, 900);
        assert_eq!(row(&r, "Read").chars, 100);
    }

    /// 서브 에이전트 결과는 별도 컨텍스트라 메인 귀속에서 제외한다.
    #[test]
    fn subagent_results_excluded_from_main_attribution() {
        let evs = vec![
            ev(r#"{"kind":"context_usage","context_tokens":10000}"#),
            ev(
                r#"{"kind":"tool_use","name":"Grep","tool_id":"s1","summary":"","parent_id":"task1"}"#,
            ),
            ev(
                r#"{"kind":"tool_result","tool_use_id":"s1","result_chars":50000,"summary":"","is_error":false,"parent_id":"task1"}"#,
            ),
            ev(r#"{"kind":"tool_use","name":"Read","tool_id":"t1","summary":""}"#),
            ev(
                r#"{"kind":"tool_result","tool_use_id":"t1","result_chars":100,"summary":"","is_error":false}"#,
            ),
            ev(r#"{"kind":"context_usage","context_tokens":10400}"#),
        ];
        let r = analyze(&evs);
        // 구간의 메인 스레드 결과는 Read 하나뿐이므로 모호하지 않다.
        assert_eq!(row(&r, "Read").attributed_tokens, 400);
        assert_eq!(r.unattributed_tokens, 0);
        assert!(r.rows.iter().all(|x| x.tool != "Grep"));
    }

    /// 컨텍스트가 줄어드는 구간(compact·캐시 경계)은 귀속하지 않는다.
    #[test]
    fn negative_delta_is_ignored() {
        let evs = vec![
            ev(r#"{"kind":"context_usage","context_tokens":90000}"#),
            ev(r#"{"kind":"tool_use","name":"Read","tool_id":"t1","summary":""}"#),
            ev(
                r#"{"kind":"tool_result","tool_use_id":"t1","result_chars":100,"summary":"","is_error":false}"#,
            ),
            ev(r#"{"kind":"context_usage","context_tokens":20000}"#),
        ];
        let r = analyze(&evs);
        assert_eq!(row(&r, "Read").attributed_tokens, 0);
        assert_eq!(r.unattributed_tokens, 0);
        assert_eq!(r.peak_context_tokens, 90000);
    }

    #[test]
    fn invalid_context_usage_clears_the_attribution_baseline() {
        let evs = vec![
            ev(r#"{"kind":"context_usage","context_tokens":1000}"#),
            ev(r#"{"kind":"tool_use","name":"Read","tool_id":"t1"}"#),
            ev(
                r#"{"kind":"tool_result","tool_use_id":"t1","result_chars":10,"summary":"","is_error":false}"#,
            ),
            ev(r#"{"kind":"context_usage","context_tokens":0,"valid":false}"#),
            ev(r#"{"kind":"context_usage","context_tokens":1500}"#),
        ];
        assert_eq!(row(&analyze(&evs), "Read").attributed_tokens, 0);
    }

    /// 툴 결과가 없는 구간의 증가분은 어시스턴트 텍스트 몫이라 미귀속으로도 세지 않는다.
    #[test]
    fn interval_without_results_is_not_counted_as_unattributed() {
        let evs = vec![
            ev(r#"{"kind":"context_usage","context_tokens":10000}"#),
            ev(r#"{"kind":"text","text":"긴 설명"}"#),
            ev(r#"{"kind":"context_usage","context_tokens":11000}"#),
        ];
        let r = analyze(&evs);
        assert_eq!(r.unattributed_tokens, 0);
        assert!(r.rows.is_empty());
    }

    /// 구 이벤트(result_chars 없음)와 비-ConvoEvent 행이 섞여도 패닉하지 않는다.
    #[test]
    fn legacy_events_without_result_chars() {
        let evs = vec![
            ev(r#"{"kind":"user","text":"안녕"}"#),
            ev(r#"{"kind":"tool_use","name":"Read","tool_id":"t1","summary":""}"#),
            ev(r#"{"kind":"tool_result","tool_use_id":"t1","summary":"...","is_error":false}"#),
            ev(r#"{"not":"kind를 가진 객체가 아님"}"#),
            ev("깨진 JSON {{{"),
        ];
        let r = analyze(&evs);
        let read = row(&r, "Read");
        assert_eq!(read.calls, 1);
        // 미상을 0으로 세면 "작은 툴"로 잘못 보인다.
        assert_eq!(read.chars, 0);
        assert_eq!(read.calls_unknown_size, 1);
    }

    /// codex는 tool_use_id를 주지 않는다 — 직전 tool_use 이름으로 귀속해야 툴별 분해가 산다.
    #[test]
    fn vendor_without_tool_ids_falls_back_to_last_tool_use() {
        let evs = vec![
            ev(r#"{"kind":"context_usage","context_tokens":1000}"#),
            ev(r#"{"kind":"tool_use","name":"shell","summary":"ls"}"#),
            ev(r#"{"kind":"tool_result","result_chars":300,"summary":"","is_error":false}"#),
            ev(r#"{"kind":"context_usage","context_tokens":1700}"#),
            ev(r#"{"kind":"tool_use","name":"edit","summary":"a.rs"}"#),
            ev(r#"{"kind":"tool_result","result_chars":40,"summary":"","is_error":false}"#),
            ev(r#"{"kind":"context_usage","context_tokens":1750}"#),
        ];
        let r = analyze(&evs);
        assert_eq!(row(&r, "shell").attributed_tokens, 700);
        assert_eq!(row(&r, "edit").attributed_tokens, 50);
        assert!(r.rows.iter().all(|x| x.tool != "(unknown)"));
    }

    /// 폴백이 서브 에이전트 tool_use에 오염되면 메인 결과가 엉뚱한 툴로 간다.
    #[test]
    fn fallback_ignores_subagent_tool_use() {
        let evs = vec![
            ev(r#"{"kind":"tool_use","name":"shell","summary":""}"#),
            ev(r#"{"kind":"tool_use","name":"Grep","summary":"","parent_id":"task1"}"#),
            ev(r#"{"kind":"tool_result","result_chars":10,"summary":"","is_error":false}"#),
        ];
        let r = analyze(&evs);
        assert_eq!(row(&r, "shell").calls, 1);
        assert!(r.rows.iter().all(|x| x.tool != "Grep"));
    }

    /// 대응하는 tool_use를 못 찾은 결과는 버리지 않고 한 버킷에 모은다.
    #[test]
    fn orphan_result_is_bucketed_not_dropped() {
        let evs = vec![ev(
            r#"{"kind":"tool_result","tool_use_id":"ghost","result_chars":10,"summary":"","is_error":false}"#,
        )];
        let r = analyze(&evs);
        assert_eq!(row(&r, "(unknown)").calls, 1);
        assert_eq!(row(&r, "(unknown)").chars, 10);
    }

    /// 귀속 토큰이 큰 순으로 정렬된다 — UI가 상위 N개만 보여주므로 순서가 계약이다.
    #[test]
    fn rows_sorted_by_attributed_then_chars() {
        let evs = vec![
            ev(r#"{"kind":"context_usage","context_tokens":1000}"#),
            ev(r#"{"kind":"tool_use","name":"Small","tool_id":"a"}"#),
            ev(
                r#"{"kind":"tool_result","tool_use_id":"a","result_chars":10,"summary":"","is_error":false}"#,
            ),
            ev(r#"{"kind":"context_usage","context_tokens":1100}"#),
            ev(r#"{"kind":"tool_use","name":"Big","tool_id":"b"}"#),
            ev(
                r#"{"kind":"tool_result","tool_use_id":"b","result_chars":20,"summary":"","is_error":false}"#,
            ),
            ev(r#"{"kind":"context_usage","context_tokens":6100}"#),
        ];
        let r = analyze(&evs);
        assert_eq!(r.rows[0].tool, "Big");
        assert_eq!(r.rows[0].attributed_tokens, 5000);
        assert_eq!(r.rows[1].tool, "Small");
    }
}
