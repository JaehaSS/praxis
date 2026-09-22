//! 린 인보케이션 테스트 — 설계 0055 §10의 T-1~T-6.
//!
//! 전부 프로세스를 띄우지 않는다. `reviewer::invocation`의 테스트(`reviewer/mod.rs:118`)와
//! 같은 이유다 — 인자 한 글자가 틀리면 CLI가 무출력으로 즉사하고, 그 실패는 `Ok(None)`에
//! 삼켜져 아무 데도 남지 않는다.

use super::invoke::*;

fn lean() -> CaptureProfile {
    CaptureProfile::default()
}

/// T-1 · 기본 프로파일의 인자 벡터와 **순서**.
///
/// 프롬프트가 `-p` 바로 뒤에 와야 한다. `--tools`는 variadic(`<tools...>`)이므로 그 뒤에
/// 위치 인자를 두면 프롬프트가 도구 이름으로 먹힌다.
#[test]
fn t1_default_profile_arg_vector() {
    let args = invocation_args(&lean(), "PROMPT");
    assert_eq!(
        args,
        vec![
            "-p",
            "PROMPT",
            "--output-format",
            "json",
            "--model",
            "sonnet",
            "--effort",
            "low",
            "--system-prompt",
            CAPTURE_SYSTEM_PROMPT,
            "--setting-sources",
            "",
            "--tools",
            "",
            "--strict-mcp-config",
            "--no-session-persistence",
        ]
    );
    // 프롬프트는 반드시 --tools보다 앞이다.
    let p = args.iter().position(|a| a == "PROMPT").unwrap();
    let t = args.iter().position(|a| a == "--tools").unwrap();
    assert!(p < t, "프롬프트가 --tools 뒤에 오면 도구 값으로 먹힌다");
}

/// T-1 · 설정 오버라이드가 실제로 인자에 실린다.
#[test]
fn t1_profile_override_reaches_args() {
    let p = CaptureProfile {
        model: "haiku".into(),
        effort: "medium".into(),
        lean: true,
    };
    let args = invocation_args(&p, "X");
    let at = |k: &str| args.iter().position(|a| a == k).unwrap();
    assert_eq!(args[at("--model") + 1], "haiku");
    assert_eq!(args[at("--effort") + 1], "medium");
}

/// T-5 · `lean=false`가 §12 롤백 집합대로만 되돌린다.
///
/// 되돌리는 것: 시스템 프롬프트 · 출력 형식 · 설정 소스.
/// 유지하는 것: 모델 · effort(상속 차단) · 도구 차단 · MCP 차단 · 세션 파일 미생성.
#[test]
fn t5_rollback_set_is_exactly_three_flags() {
    let p = CaptureProfile {
        lean: false,
        ..CaptureProfile::default()
    };
    let args = invocation_args(&p, "X");
    let has = |k: &str| args.iter().any(|a| a == k);

    assert!(!has("--system-prompt"), "롤백: 시스템 프롬프트 제거");
    assert!(!has("--setting-sources"), "롤백: 설정 소스 복원");
    let of = args.iter().position(|a| a == "--output-format").unwrap();
    assert_eq!(args[of + 1], "text", "롤백: 출력 형식");

    assert!(has("--model"), "유지: 상속 차단은 롤백 대상이 아니다");
    assert!(has("--effort"), "유지: 상동");
    assert!(has("--tools"), "유지: 신뢰 경계는 비용 결정이 아니다");
    assert!(has("--strict-mcp-config"), "유지: 상동");
    assert!(has("--no-session-persistence"), "유지: 파일 위생");
}

/// T-3 · 실패 봉투가 실패로 접히는지.
///
/// 픽스처는 실측값이다(CLI 2.1.258, `--model sonnet-typo-xyz`). **`subtype`이 `"success"`인데
/// `is_error`가 `true`다** — `subtype`으로 판정하면 이 실패를 놓치고 오류 문장이 회고가 된다.
#[test]
fn t3_is_error_envelope_with_success_subtype_is_a_failure() {
    let fixture = r#"{
      "type":"result","subtype":"success","is_error":true,
      "terminal_reason":"api_error","api_error_status":404,
      "result":"There's an issue with the selected model (sonnet-typo-xyz).",
      "total_cost_usd":0,
      "usage":{"input_tokens":0,"output_tokens":0}
    }"#;
    let env = parse_envelope(fixture).expect("봉투는 파싱된다");
    assert!(env.is_error, "is_error가 주 판정이다");
    assert_eq!(env.diagnostic.as_deref(), Some("api_error (404)"));
    // subtype이 success라는 사실 자체를 고정해 둔다 — 이것이 이 테스트의 존재 이유다.
    let v: serde_json::Value = serde_json::from_str(fixture).unwrap();
    assert_eq!(v["subtype"], "success");
}

/// T-3 · 정상 봉투에서 텍스트와 usage를 회수한다.
#[test]
fn t3_success_envelope_yields_text_and_usage() {
    let fixture = r#"{"type":"result","subtype":"success","is_error":false,
      "result":"교훈 한 문장.","total_cost_usd":0.0299,
      "usage":{"input_tokens":10,"output_tokens":537}}"#;
    let env = parse_envelope(fixture).unwrap();
    assert_eq!(env.text, "교훈 한 문장.");
    assert!(!env.is_error);
    assert_eq!(env.cost_usd, Some(0.0299));
    assert_eq!(env.input_tokens, Some(10));
    assert_eq!(env.output_tokens, Some(537));
}

/// T-3 · 봉투가 아닌 출력은 파싱 실패로 접힌다(계약 3).
#[test]
fn t3_non_envelope_output_is_rejected() {
    assert!(parse_envelope("그냥 텍스트 한 줄").is_none());
    assert!(parse_envelope("").is_none());
    // result 필드가 없는 JSON도 거부 — 부분 파싱으로 통과시키지 않는다.
    assert!(parse_envelope(r#"{"is_error":false}"#).is_none());
}

/// T-2 · 봉투 → `result` → `split_section` 왕복.
///
/// 봉투 안에서 개행은 `\n`으로 이스케이프돼 **한 줄**이 된다. `split_section`은 줄 단위
/// 파서이므로, 봉투 원문을 그대로 넘기면 `PRAXIS_CITATIONS:`를 영영 못 찾는다.
#[test]
fn t2_citation_survives_envelope_round_trip() {
    let inner = "[{\"kind\":\"decision\",\"content\":\"x\"}]\nPRAXIS_CITATIONS: {\"cited\":[1,2]}";
    let envelope = serde_json::json!({
        "type": "result", "subtype": "success", "is_error": false,
        "result": inner,
    })
    .to_string();

    // 봉투 원문을 그대로 주면 실패한다 — 이 변경이 막으려는 실패 모드.
    let (_, direct) = crate::memory::citation::split_section(&envelope);
    assert!(direct.is_none(), "봉투 원문에서는 섹션을 찾을 수 없다");

    // 계약대로 result를 꺼내 넘기면 찾는다.
    let env = parse_envelope(&envelope).unwrap();
    let (cleaned, section) = crate::memory::citation::split_section(&env.text);
    assert_eq!(section.as_deref(), Some(r#"{"cited":[1,2]}"#));
    assert!(cleaned.contains("decision"), "배열은 정화본에 남는다");
}

/// T-6 · 추출 기록이 뒤따르는 회고 기록에 덮이지 않는다.
///
/// 한 작업에서 추출 다음에 회고가 돈다. 슬롯이 하나면 추출의 `parsed_ok`·`citation_found`가
/// 언제나 사라지고, 그러면 관측 설계가 자기 관측을 덮는다.
#[test]
fn t6_reflect_record_does_not_overwrite_extract() {
    clear_runs();
    let p = lean();
    let mk = |ok: bool| CaptureRun {
        model: p.model.clone(),
        effort: p.effort.clone(),
        lean: true,
        cost_usd: Some(0.04),
        input_tokens: None,
        output_tokens: None,
        ok,
        parsed_ok: false,
        citation_found: None,
        err: None,
        at: 0,
    };

    // 실제 순서: 추출 → (파싱 결과 통보) → 회고
    record(CaptureKind::Extract, mk(true));
    mark_parsed(CaptureKind::Extract, true, Some(true));
    record(CaptureKind::Reflect, mk(true));
    mark_parsed(CaptureKind::Reflect, true, None);

    let runs = last_runs();
    assert_eq!(runs.len(), 2, "두 kind가 각자 슬롯을 갖는다");

    let extract = runs.get("extract").expect("추출 기록이 살아 있어야 한다");
    assert!(extract.parsed_ok, "추출의 파싱 결과가 보존된다");
    assert_eq!(
        extract.citation_found,
        Some(true),
        "인용 판정 결과가 회고에 덮이지 않는다"
    );
    assert_eq!(
        runs.get("reflect").unwrap().citation_found,
        None,
        "회고는 인용을 요청하지 않으므로 None"
    );
}

async fn settings_pool() -> (sqlx::SqlitePool, String) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir()
        .join(format!(
            "praxis-invoke-test-{}-{n}.sqlite",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned();
    let pool = crate::db::init_pool(&path).await.expect("init pool");
    (pool, path)
}

/// T-4 · 미설정·빈값·무효는 전부 코드 상수로 접힌다.
///
/// **CLI 기본값으로 떨어지게 두지 않는 것**이 이 작업의 요점이다 — 명시하지 않은 것은
/// 언젠가 사용자의 최상위 모델이 되고, 그 상속은 조용하다.
#[tokio::test]
async fn t4_unset_blank_and_invalid_all_fold_to_constants() {
    let (pool, path) = settings_pool().await;

    // 미설정
    let p = profile(&pool).await;
    assert_eq!(p.model, DEFAULT_MODEL);
    assert_eq!(p.effort, DEFAULT_EFFORT);
    assert!(p.lean, "미설정은 린이 기본이다");

    // 공백만 있는 값도 미설정으로 본다.
    crate::db::set_setting(&pool, KEY_MODEL, "   ").await.unwrap();
    crate::db::set_setting(&pool, KEY_EFFORT, "  ").await.unwrap();
    let p = profile(&pool).await;
    assert_eq!(p.model, DEFAULT_MODEL);
    assert_eq!(p.effort, DEFAULT_EFFORT);

    // 무효 effort는 검증기에 걸려 상수로 접힌다 — 그대로 넘기면 CLI가 즉사한다.
    crate::db::set_setting(&pool, KEY_EFFORT, "ultra").await.unwrap();
    assert_eq!(profile(&pool).await.effort, DEFAULT_EFFORT);

    // 유효값은 그대로 실린다.
    crate::db::set_setting(&pool, KEY_MODEL, "haiku").await.unwrap();
    crate::db::set_setting(&pool, KEY_EFFORT, "medium").await.unwrap();
    let p = profile(&pool).await;
    assert_eq!(p.model, "haiku");
    assert_eq!(p.effort, "medium");

    // lean은 명시적 "false"일 때만 꺼진다 — 오타가 롤백을 유발하면 안 된다.
    crate::db::set_setting(&pool, KEY_LEAN, "nope").await.unwrap();
    assert!(profile(&pool).await.lean, "\"false\"가 아니면 린 유지");
    crate::db::set_setting(&pool, KEY_LEAN, "false").await.unwrap();
    assert!(!profile(&pool).await.lean);

    pool.close().await;
    let _ = std::fs::remove_file(&path);
}

/// 기본값 상수가 카탈로그·검증기와 어긋나지 않는지.
///
/// `sonnet`은 `src/lib/models.ts`의 claude 별칭이고, `low`는 `reasoning_effort_override`가
/// 허용하는 최저값이다. 둘 중 하나가 어긋나면 첫 호출이 즉사한다.
#[test]
fn defaults_are_accepted_by_existing_validators() {
    assert_eq!(
        crate::agent::reasoning_effort_override("claude", Some(DEFAULT_EFFORT)),
        Ok(Some(DEFAULT_EFFORT.to_string()))
    );
    assert_eq!(DEFAULT_MODEL, "sonnet");
}
