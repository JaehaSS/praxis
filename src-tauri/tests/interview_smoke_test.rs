//! 인터뷰 수동 스모크 (Plan 0021 Task 8.3) — 실제 헤드리스 CLI를 호출하므로 기본 제외.
//! 실행: `cargo test --test interview_smoke_test -- --ignored --nocapture`
//! 모델 출력은 비결정적이므로 구조 검증(파싱 성공·점수 범위)만 단언하고 판단 결과는 출력한다.

use praxis_lib::{interview, reviewer};

fn nonce() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(std::process::id() as u64);
    format!("PRAXIS-INTERVIEW-SMOKE-{:016x}", h.finish())
}

fn run_assessment_with_vendor(instruction: &str, vendor: &str) -> interview::InterviewAssessment {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    let ctx = interview::collect_repo_context(&repo).expect("레포 컨텍스트 수집");
    let nonce = nonce();
    let prompt = interview::build_assessment_prompt(&ctx, instruction, &nonce);
    eprintln!("[smoke] 리뷰어 벤더: {vendor}");
    let raw = reviewer::run_reviewer(vendor, &prompt, interview::INTERVIEW_TIMEOUT_SECS)
        .expect("리뷰어 실행");
    interview::parse_assessment(&raw, &nonce).expect("assessment 파싱")
}

fn run_assessment(instruction: &str) -> interview::InterviewAssessment {
    let vendor = reviewer::detect_reviewer("");
    run_assessment_with_vendor(instruction, &vendor)
}

#[test]
#[ignore = "실제 agy CLI 호출 — 수동 스모크 전용"]
fn smoke_agy_preserves_interview_verification_token() {
    let assessment = run_assessment_with_vendor("메모리 화면을 더 좋게 개선해줘", "agy");
    assert!(assessment.questions.len() <= 5);
}

#[test]
#[ignore = "실제 CLI 호출 — 수동 스모크 전용"]
fn smoke_ambiguous_instruction_yields_questions() {
    let a = run_assessment("메모리 화면을 더 좋게 개선해줘");
    let s = &a.ambiguity;
    for v in [s.score, s.goal, s.constraints, s.success] {
        assert!((0.0..=1.0).contains(&v), "점수 범위: {v}");
    }
    assert!(a.questions.len() <= 5);
    eprintln!(
        "[smoke] 모호 지시문 → score={:.2} (goal {:.2}/constraints {:.2}/success {:.2}), 질문 {}개",
        s.score,
        s.goal,
        s.constraints,
        s.success,
        a.questions.len()
    );
    for q in &a.questions {
        eprintln!("  - [{}] {} ({}보기)", q.dimension, q.text, q.options.len());
    }
}

#[test]
#[ignore = "실제 CLI 호출 — 수동 스모크 전용"]
fn smoke_clear_instruction_reports_skip_decision() {
    let a = run_assessment(
        "README.md 첫 문단의 오타 'teh'를 'the'로 수정해줘. 다른 파일은 건드리지 말 것. 완료 기준은 해당 오타가 사라지는 것.",
    );
    let skip = interview::should_skip_questions(&a.ambiguity);
    eprintln!(
        "[smoke] 명확 지시문 → score={:.2}, 질문 생략 판정={skip}, 질문 {}개",
        a.ambiguity.score,
        a.questions.len()
    );
}
