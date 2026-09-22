//! 라이브 모델 E2E — Praxis의 실제 reviewer 경로(temp-cwd + stdin + timeout/killpg)로
//! 3개 벤더(claude / codex / agy=Antigravity·Gemini 백엔드)를 각각 1회 호출해 답이 오는지 확인.
//! agy는 자체 로그인 인증이라 gemini API 키 없이 Gemini 모델로 답한다(키 없는 환경 대응).
//!
//! 네트워크 + 실제 CLI 인증이 필요하므로 기본 제외(#[ignore]).
//! 실행: PATH에 Node 24 + ~/.local/bin prepend 후
//!       `cargo test --test live_models_test -- --ignored --nocapture`

use praxis_lib::reviewer::run_reviewer;

#[ignore]
#[test]
fn live_agy_receives_print_flag_prompt_value() {
    let marker = "PRAXIS-AGY-PROMPT-DELIVERY-OK";
    let prompt = format!("Reply with ONLY this exact text: {marker}");
    let output = run_reviewer("agy", &prompt, 90).expect("agy reviewer 실행");
    assert_eq!(output.trim(), marker);
}

#[ignore]
#[test]
fn live_three_models_answer() {
    let prompt = "What is 2+2? Reply with ONLY the number, nothing else.";
    // 각 모델 90s 제한(run_reviewer가 타임아웃 시 프로세스 그룹 kill). agy는 콜드 스타트가 느릴 수 있음.
    for model in ["claude", "codex", "agy"] {
        let r = run_reviewer(model, prompt, 90);
        match &r {
            Ok(out) => {
                let ans = out.trim();
                println!(
                    "[{model}] OK ({}자): {}",
                    ans.len(),
                    ans.lines().next().unwrap_or("")
                );
            }
            Err(e) => println!("[{model}] ERR: {e}"),
        }
    }

    // 최소 보증: claude는 반드시 답(파이프라인 살아있음). codex/agy는 환경 인증 의존이라 보고만.
    let claude = run_reviewer("claude", prompt, 90);
    assert!(
        claude
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        "claude reviewer 경로가 답을 반환해야 함: {claude:?}"
    );
}
