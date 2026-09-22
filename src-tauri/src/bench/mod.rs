//! 실험 통계 — 고정 태스크 셋을 변형별로 반복 실행했을 때의 **집계와 신뢰구간**.
//! Tauri 비의존(순수 계획/집계 + `cargo test`).
//!
//! # 실행부는 없다
//!
//! 실험 탭(UI·Tauri 커맨드)은 걷어냈다 — 11일간 실행 0회였다(ADR 0136). 남은 것은 계획을
//! 표현·검증하는 타입과 집계·구간 계산이고, 현재 호출자는 테스트뿐이다. 측정이 돌아오면
//! 여기서 다시 쓴다. 없앨 것은 쓰지 않는 화면이지 통계가 아니다.
//!
//! # 이것은 유의한 벤치마크가 아니다
//!
//! 태스크 5개 × 3회로는 승률 60%와 70%를 구별할 수 없다(`wilson` 테스트가 이를 못 박는다).
//! 용도는 **회귀 탐지기**다 — 미세한 승률 차이가 아니라 명백한 실패 모드(파싱 실패, 도구 오용,
//! 무한 루프, 게이트 0% 통과)를 잡는다. 결과에 신뢰구간을 함께 실어 작은 차이를 과신하지 않게 한다.
//!
//! `insights`/`ensemble::metrics`와 겹치지 않는다. 그쪽은 실제 작업의 **사후 관측**이라 작업
//! 난이도가 교란변수로 섞인다 — "하네스를 바꿔서 나아졌나"에 답하지 못한다. 여기는 고정 태스크를
//! 쓰는 통제 실험이다.
//!
//! 채점에 LLM 심판을 쓰지 않는다. `verify::gate`가 이미 자동 채점기다 — 빌드·테스트 통과가 곧
//! 성공이라 심판 비용도 심판 편향도 없다.

use serde::{Deserialize, Serialize};

mod wilson;
pub use wilson::interval;

/// 골든 태스크. `.praxis/bench/*.json`에서 로드한다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchTask {
    pub id: String,
    pub instruction: String,
    pub repo: String,
    #[serde(default)]
    pub base: Option<String>,
}

/// 하네스 변형. 1차 용도는 `turn_guard` 프롬프트를 바꿔가며 재는 것이다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Variant {
    pub label: String,
    pub vendor: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub prompt_preset: Option<String>,
}

/// 한 판의 계획.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchPlan {
    pub tasks: Vec<BenchTask>,
    pub variants: Vec<Variant>,
    pub repeats: u32,
}

/// 실행 전 사용자에게 보여줄 규모. 이것을 확인해야 실행이 시작된다 — 실행이 곧 쿼터 소모다.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RunEstimate {
    pub turns: u32,
    pub tasks: usize,
    pub variants: usize,
    pub repeats: u32,
}

/// 실행 1회의 결과.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RunResult {
    pub variant: String,
    pub task_id: String,
    pub iteration: u32,
    pub passed: bool,
    pub tokens: Option<i64>,
    pub seconds: f64,
}

/// 변형별 집계.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VariantSummary {
    pub label: String,
    pub runs: u32,
    pub passed: u32,
    pub pass_rate: f64,
    /// Wilson score interval — 작은 표본에서 정규근사보다 정직하다.
    pub ci_low: f64,
    pub ci_high: f64,
    /// 토큰을 보고하지 않은 실행이 섞이면 보고한 것만 평균낸다. 전부 없으면 `None`.
    pub avg_tokens: Option<f64>,
    pub avg_seconds: f64,
}

/// 계획 파일 본문을 읽어 검증까지 마친다. 손으로 쓰는 파일이라 형식 오류가 흔하다 —
/// 오류 메시지에 기대 형식을 함께 실어 돌려준다.
pub fn parse_plan(raw: &str) -> Result<BenchPlan, String> {
    let plan: BenchPlan = serde_json::from_str(raw).map_err(|e| {
        format!(
            "계획 파일 형식이 올바르지 않습니다: {e}\n\
             기대 형식: {{\"tasks\": [{{\"id\": \"…\", \"instruction\": \"…\", \"repo\": \"…\"}}], \
             \"variants\": [{{\"label\": \"…\", \"vendor\": \"claude\"}}], \"repeats\": 3}}"
        )
    })?;
    validate(&plan)?;
    Ok(plan)
}

/// 계획 한 판이 몇 턴인지. 이 수치가 곧 LLM 호출 횟수다.
pub fn estimate(plan: &BenchPlan) -> RunEstimate {
    let tasks = plan.tasks.len();
    let variants = plan.variants.len();
    RunEstimate {
        turns: (tasks as u32) * (variants as u32) * plan.repeats,
        tasks,
        variants,
        repeats: plan.repeats,
    }
}

/// 계획이 실행 가능한지. 빈 축이 있으면 0턴짜리 실행이 조용히 성공하는 것을 막는다.
pub fn validate(plan: &BenchPlan) -> Result<(), String> {
    if plan.tasks.is_empty() {
        return Err("골든 태스크가 없습니다".into());
    }
    if plan.variants.is_empty() {
        return Err("비교할 변형이 없습니다".into());
    }
    if plan.repeats == 0 {
        return Err("반복 횟수는 1 이상이어야 합니다".into());
    }
    let mut labels: Vec<&str> = plan.variants.iter().map(|v| v.label.as_str()).collect();
    labels.sort_unstable();
    if labels.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("변형 라벨이 중복됩니다 — 집계가 섞입니다".into());
    }
    let mut ids: Vec<&str> = plan.tasks.iter().map(|t| t.id.as_str()).collect();
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("골든 태스크 id가 중복됩니다".into());
    }
    Ok(())
}

/// 변형별로 집계한다. 결과가 없는 변형은 나오지 않는다.
/// 순서는 통과율 내림차순 — 같으면 라벨 오름차순으로 안정화한다.
pub fn summarize(results: &[RunResult]) -> Vec<VariantSummary> {
    let mut labels: Vec<&str> = results.iter().map(|r| r.variant.as_str()).collect();
    labels.sort_unstable();
    labels.dedup();

    let mut summaries: Vec<VariantSummary> = labels
        .into_iter()
        .map(|label| {
            let rows: Vec<&RunResult> = results.iter().filter(|r| r.variant == label).collect();
            let runs = rows.len() as u32;
            let passed = rows.iter().filter(|r| r.passed).count() as u32;
            let (ci_low, ci_high) = interval(passed, runs);
            let token_rows: Vec<i64> = rows.iter().filter_map(|r| r.tokens).collect();
            let avg_tokens = (!token_rows.is_empty()).then(|| {
                token_rows.iter().sum::<i64>() as f64 / token_rows.len() as f64
            });
            let avg_seconds = if runs == 0 {
                0.0
            } else {
                rows.iter().map(|r| r.seconds).sum::<f64>() / f64::from(runs)
            };
            VariantSummary {
                label: label.to_string(),
                runs,
                passed,
                pass_rate: if runs == 0 {
                    0.0
                } else {
                    f64::from(passed) / f64::from(runs)
                },
                ci_low,
                ci_high,
                avg_tokens,
                avg_seconds,
            }
        })
        .collect();
    summaries.sort_by(|a, b| {
        b.pass_rate
            .partial_cmp(&a.pass_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.label.cmp(&b.label))
    });
    summaries
}

/// 두 변형의 신뢰구간이 겹치는가 — 겹치면 **차이를 주장할 수 없다**.
/// 결과를 보고하는 쪽이 "우열을 가릴 수 없음"을 말하는 근거다.
pub fn indistinguishable(a: &VariantSummary, b: &VariantSummary) -> bool {
    a.ci_low <= b.ci_high && b.ci_low <= a.ci_high
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str) -> BenchTask {
        BenchTask {
            id: id.to_string(),
            instruction: "무언가 하라".into(),
            repo: "/tmp/repo".into(),
            base: None,
        }
    }

    fn variant(label: &str) -> Variant {
        Variant {
            label: label.to_string(),
            vendor: "claude".into(),
            model: None,
            effort: None,
            prompt_preset: None,
        }
    }

    fn run(variant: &str, task_id: &str, iteration: u32, passed: bool) -> RunResult {
        RunResult {
            variant: variant.to_string(),
            task_id: task_id.to_string(),
            iteration,
            passed,
            tokens: Some(1000),
            seconds: 10.0,
        }
    }

    #[test]
    fn the_estimate_is_the_product_of_all_three_axes() {
        let plan = BenchPlan {
            tasks: vec![task("a"), task("b"), task("c")],
            variants: vec![variant("기준"), variant("변경")],
            repeats: 4,
        };
        let estimate = estimate(&plan);
        assert_eq!(estimate.turns, 24);
        assert_eq!(estimate.tasks, 3);
        assert_eq!(estimate.variants, 2);
        assert_eq!(estimate.repeats, 4);
    }

    #[test]
    fn an_empty_axis_is_rejected() {
        let base = BenchPlan {
            tasks: vec![task("a")],
            variants: vec![variant("기준")],
            repeats: 1,
        };
        assert!(validate(&base).is_ok());

        let mut no_tasks = base.clone();
        no_tasks.tasks.clear();
        assert!(validate(&no_tasks).is_err());

        let mut no_variants = base.clone();
        no_variants.variants.clear();
        assert!(validate(&no_variants).is_err());

        let mut no_repeats = base.clone();
        no_repeats.repeats = 0;
        assert!(validate(&no_repeats).is_err());
    }

    #[test]
    fn duplicate_labels_are_rejected_because_they_merge_silently() {
        let plan = BenchPlan {
            tasks: vec![task("a")],
            variants: vec![variant("같은이름"), variant("같은이름")],
            repeats: 1,
        };
        let err = validate(&plan).unwrap_err();
        assert!(err.contains("중복"), "예상과 다른 거부: {err}");
    }

    #[test]
    fn duplicate_task_ids_are_rejected() {
        let plan = BenchPlan {
            tasks: vec![task("같은id"), task("같은id")],
            variants: vec![variant("기준")],
            repeats: 1,
        };
        assert!(validate(&plan).is_err());
    }

    #[test]
    fn summaries_count_passes_per_variant() {
        let results = vec![
            run("A", "t1", 0, true),
            run("A", "t2", 0, false),
            run("B", "t1", 0, true),
            run("B", "t2", 0, true),
        ];
        let summaries = summarize(&results);
        assert_eq!(summaries.len(), 2);
        // 통과율 내림차순 — B(2/2)가 A(1/2)보다 앞.
        assert_eq!(summaries[0].label, "B");
        assert_eq!(summaries[0].passed, 2);
        assert_eq!(summaries[0].runs, 2);
        assert!((summaries[0].pass_rate - 1.0).abs() < 1e-9);
        assert_eq!(summaries[1].label, "A");
        assert!((summaries[1].pass_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn missing_token_counts_do_not_poison_the_average() {
        let results = vec![
            RunResult {
                tokens: Some(100),
                ..run("A", "t1", 0, true)
            },
            RunResult {
                tokens: None,
                ..run("A", "t2", 0, true)
            },
        ];
        let summaries = summarize(&results);
        // 보고한 한 건의 평균이어야 한다 — None을 0으로 세면 50이 된다.
        assert_eq!(summaries[0].avg_tokens, Some(100.0));
    }

    #[test]
    fn a_variant_with_no_token_reports_has_no_average() {
        let results = vec![RunResult {
            tokens: None,
            ..run("A", "t1", 0, true)
        }];
        assert_eq!(summarize(&results)[0].avg_tokens, None);
    }

    #[test]
    fn empty_results_summarize_to_nothing() {
        assert!(summarize(&[]).is_empty());
    }

    #[test]
    fn overlapping_intervals_mean_no_claim_can_be_made() {
        // 6/10 대 7/10 — 이 표본으로는 우열을 가릴 수 없다.
        let results = [
            (0..10)
                .map(|i| run("A", &format!("t{i}"), 0, i < 6))
                .collect::<Vec<_>>(),
            (0..10)
                .map(|i| run("B", &format!("t{i}"), 0, i < 7))
                .collect::<Vec<_>>(),
        ]
        .concat();
        let summaries = summarize(&results);
        assert!(indistinguishable(&summaries[0], &summaries[1]));
    }

    #[test]
    fn a_blowout_is_distinguishable() {
        // 0/20 대 20/20 — 이 정도 차이는 회귀 탐지기로도 잡힌다.
        let results = [
            (0..20)
                .map(|i| run("망가짐", &format!("t{i}"), 0, false))
                .collect::<Vec<_>>(),
            (0..20)
                .map(|i| run("정상", &format!("t{i}"), 0, true))
                .collect::<Vec<_>>(),
        ]
        .concat();
        let summaries = summarize(&results);
        assert!(!indistinguishable(&summaries[0], &summaries[1]));
    }

    #[test]
    fn a_plan_round_trips_through_json() {
        let plan = BenchPlan {
            tasks: vec![task("a")],
            variants: vec![variant("기준")],
            repeats: 2,
        };
        let json = serde_json::to_string(&plan).unwrap();
        assert_eq!(serde_json::from_str::<BenchPlan>(&json).unwrap(), plan);
    }

    /// 손으로 쓰는 파일이므로 선택 필드를 빼먹어도 읽혀야 한다.
    #[test]
    fn optional_fields_may_be_omitted_in_the_file() {
        let json = r#"{
            "tasks": [{"id": "a", "instruction": "하라", "repo": "/tmp/r"}],
            "variants": [{"label": "기준", "vendor": "claude"}],
            "repeats": 1
        }"#;
        let plan: BenchPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.tasks[0].base, None);
        assert_eq!(plan.variants[0].model, None);
        assert!(validate(&plan).is_ok());
    }

    #[test]
    fn a_realistic_plan_file_parses() {
        let json = r#"{
            "tasks": [
                {
                    "id": "add-test",
                    "instruction": "src/lib.rs의 add 함수에 경계값 테스트를 추가하라.",
                    "repo": "/tmp/fixture",
                    "base": "main"
                },
                {
                    "id": "fix-clippy",
                    "instruction": "clippy 경고를 모두 없애라.",
                    "repo": "/tmp/fixture"
                }
            ],
            "variants": [
                {"label": "기준 프롬프트", "vendor": "claude"},
                {"label": "턴가드 강화", "vendor": "claude", "prompt_preset": "strict-turn-guard"}
            ],
            "repeats": 3
        }"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(estimate(&plan).turns, 12);
        assert_eq!(plan.tasks[0].base.as_deref(), Some("main"));
        assert_eq!(
            plan.variants[1].prompt_preset.as_deref(),
            Some("strict-turn-guard")
        );
    }

    /// 형식 오류 메시지는 기대 형식을 함께 보여야 한다 — 손으로 고칠 파일이다.
    #[test]
    fn a_malformed_file_explains_the_expected_shape() {
        let err = parse_plan("{ not json").unwrap_err();
        assert!(err.contains("기대 형식"), "형식 힌트가 없다: {err}");
    }

    /// 문법은 맞지만 실행할 수 없는 계획은 파싱 단계에서 걸린다.
    #[test]
    fn a_valid_json_with_an_empty_axis_is_still_rejected() {
        let json = r#"{"tasks": [], "variants": [], "repeats": 1}"#;
        assert!(parse_plan(json).is_err());
    }
}
