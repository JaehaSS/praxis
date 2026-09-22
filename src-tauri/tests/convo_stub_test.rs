//! 대화 벤더 I/O·파서 헤르메틱 검증 — 실제 CLI 없이 스텁 바이너리로 `run_turn`을 돌린다.
//! `bin` 주입(호출측이 실행 파일 경로 지정) 덕에 core convo를 결정적으로 테스트(리뷰 arch 지적 해소).
//! 스텁은 셸 스크립트라 unix 전용(대상 로직 run_turn 자체는 크로스플랫폼).

#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::convo::{run_turn, ConvoEvent, Vendor};

static N: AtomicU32 = AtomicU32::new(0);

/// 임의 셸 스크립트 본문으로 실행 스텁을 임시 경로에 생성 → 절대 경로 반환.
/// `body`가 `#!/bin/sh\n` 뒤에 그대로 들어간다 — sleep 등 고정 printf만으론 표현 못 할 타이밍 테스트용.
fn stub_script(body: &str) -> (std::path::PathBuf, String) {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = temp_root::dir().join(format!("praxis-stub-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("vendor-stub");
    let mut f = std::fs::File::create(&bin).unwrap();
    write!(f, "#!/bin/sh\n{body}\n").unwrap();
    let mut perm = f.metadata().unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&bin, perm).unwrap();
    (dir, bin.to_string_lossy().into_owned())
}

/// 고정 출력을 내는 실행 스텁 스크립트를 임시 경로에 생성 → 절대 경로 반환.
fn stub(printf_args: &str) -> (std::path::PathBuf, String) {
    stub_script(&format!("printf '%s\\n' {printf_args}"))
}

fn run(vendor: Vendor, bin: &str) -> (Result<String, String>, Vec<ConvoEvent>) {
    run_with_idle_timeout(vendor, bin, 5)
}

/// `run`의 유휴 타임아웃 조정판 — 하트비트 리셋/유휴 kill 타이밍 검증 전용.
fn run_with_idle_timeout(
    vendor: Vendor,
    bin: &str,
    idle_timeout_secs: u64,
) -> (Result<String, String>, Vec<ConvoEvent>) {
    run_with_model_and_idle_timeout(vendor, bin, None, idle_timeout_secs)
}

fn run_with_model_and_idle_timeout(
    vendor: Vendor,
    bin: &str,
    model: Option<&str>,
    idle_timeout_secs: u64,
) -> (Result<String, String>, Vec<ConvoEvent>) {
    let cwd = temp_root::dir();
    let mut evs = Vec::new();
    let res = run_turn(
        &cwd.to_string_lossy(),
        "hi",
        None,
        idle_timeout_secs,
        vendor,
        bin,
        model,
        |_| {},
        |e| evs.push(e),
    )
    .map(|o| o.session_id); // 이 테스트들의 관심사는 resume 토큰 — 종료 관측은 unit test에서 검증.
    (res, evs)
}

#[test]
fn claude_stream_json_stub_parses_session_text_result() {
    let (dir, bin) = stub(
        "'{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s-stub\"}' \
         '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}' \
         '{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done\",\"session_id\":\"s-stub\",\"total_cost_usd\":0.01,\"num_turns\":1}'",
    );
    let (res, evs) = run(Vendor::Claude, &bin);
    assert_eq!(res.as_deref(), Ok("s-stub"));
    assert!(evs
        .iter()
        .any(|e| matches!(e, ConvoEvent::SessionInit { session_id } if session_id == "s-stub")));
    assert!(evs
        .iter()
        .any(|e| matches!(e, ConvoEvent::Text { text, .. } if text == "hello")));
    assert!(evs.iter().any(
        |e| matches!(e, ConvoEvent::Result { cost_usd, .. } if (*cost_usd - 0.01).abs() < 1e-9)
    ));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn claude_turn_records_requested_and_observed_models() {
    let (dir, bin) = stub(
        "'{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s-model\",\"model\":\"claude-opus-4-8\"}' \
         '{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done\",\"session_id\":\"s-model\"}'",
    );

    let (res, events) = run_with_model_and_idle_timeout(Vendor::Claude, &bin, Some("opus"), 5);

    assert_eq!(res.as_deref(), Ok("s-model"));
    assert!(events.iter().any(|event| matches!(
        event,
        ConvoEvent::ModelSnapshot {
            requested: Some(requested),
            resolved: None,
            source,
        } if requested == "opus" && source == "invocation"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ConvoEvent::ModelSnapshot {
            requested: None,
            resolved: Some(resolved),
            source,
        } if resolved == "claude-opus-4-8" && source == "claude_stream"
    )));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn codex_jsonl_stub_parses_session_text_result() {
    let (dir, bin) = stub(
        "'{\"type\":\"thread.started\",\"thread_id\":\"t-stub\"}' \
         '{\"type\":\"item.completed\",\"item\":{\"id\":\"i0\",\"type\":\"agent_message\",\"text\":\"OK\"}}' \
         '{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":20,\"output_tokens\":3}}'",
    );
    let (res, evs) = run(Vendor::Codex, &bin);
    assert_eq!(res.as_deref(), Ok("t-stub"));
    assert!(evs
        .iter()
        .any(|e| matches!(e, ConvoEvent::SessionInit { session_id } if session_id == "t-stub")));
    assert!(evs
        .iter()
        .any(|e| matches!(e, ConvoEvent::Text { text, .. } if text == "OK")));
    assert!(evs
        .iter()
        .any(|e| matches!(e, ConvoEvent::Result { tokens_in, .. } if *tokens_in == 20)));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn agy_plaintext_stub_buffers_one_text_and_synthetic_result() {
    let (dir, bin) = stub("'first line' 'second line'");
    let (res, evs) = run(Vendor::Agy, &bin);
    assert_eq!(
        res.as_deref(),
        Ok("continue"),
        "agy는 --continue 센티널 반환"
    );
    // 전체 출력이 Text 1블록으로 합쳐져야(구조화 스트림 없음).
    let texts: Vec<_> = evs
        .iter()
        .filter_map(|e| match e {
            ConvoEvent::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 1, "정확히 하나의 Text 블록: {evs:?}");
    assert!(texts[0].contains("first line") && texts[0].contains("second line"));
    assert!(evs
        .iter()
        .any(|e| matches!(e, ConvoEvent::Result { session_id, .. } if session_id == "continue")));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn agy_reaps_an_exited_cli_even_when_a_detached_child_keeps_stdout_open() {
    let script = [
        "/usr/bin/python3 -c 'import os,time; os.setsid(); time.sleep(3)' &",
        "printf '%s\\n' 'completed before detached child'",
    ]
    .join("\n");
    let (dir, bin) = stub_script(&script);
    let started = std::time::Instant::now();

    let (res, events) = run_with_idle_timeout(Vendor::Agy, &bin, 5);

    assert!(
        res.is_ok(),
        "CLI 본체의 정상 종료는 성공으로 회수해야 함: {res:?}"
    );
    // 경계는 자식 수명(3초)보다 작아야 회귀를 잡는다 — 매달렸다면 자식이 죽어 stdout이
    // 닫힐 때까지 끌려간다. 자식을 늘려 여유를 만들려 했으나, 살아남은 detached 프로세스가
    // 이웃 테스트의 프로세스 스캔에 섞여 더 나빴다.
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "분리된 자식이 stdout을 잡고 있어도 CLI 본체 종료 시 즉시 회수해야 함: {elapsed:?}"
    );
    assert!(events.iter().any(
        |event| matches!(event, ConvoEvent::Result { session_id, .. } if session_id == "continue")
    ));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn active_turn_survives_total_time_beyond_idle_cap() {
    // 매 라인이 하트비트를 리셋하므로, 개별 무출력 구간(1s)이 유휴 상한(3s)보다 작으면
    // 총 소요(~4s)가 상한을 넘어도 살아남아야 한다 — "턴 전체 타임아웃"이 아님을 검증.
    let script = [
        "printf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s-idle\"}'",
        "sleep 1",
        "printf '%s\\n' '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"tick1\"}]}}'",
        "sleep 1",
        "printf '%s\\n' '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"tick2\"}]}}'",
        "sleep 1",
        "printf '%s\\n' '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"tick3\"}]}}'",
        "sleep 1",
        "printf '%s\\n' '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"tick4\"}]}}'",
        "printf '%s\\n' '{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done\",\"session_id\":\"s-idle\",\"total_cost_usd\":0.0,\"num_turns\":1}'",
    ]
    .join("\n");
    let (dir, bin) = stub_script(&script);
    let (res, evs) = run_with_idle_timeout(Vendor::Claude, &bin, 3);
    assert!(
        res.is_ok(),
        "라인 간격(1s) < 유휴 상한(3s)이면 총 소요가 상한을 넘어도 생존해야 함: {res:?}"
    );
    assert!(
        evs.iter().any(|e| matches!(e, ConvoEvent::Result { .. })),
        "정상 종료면 Result 이벤트가 있어야 함: {evs:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn hung_turn_is_killed_at_idle_cap_without_waiting_for_full_sleep() {
    // init 후 30s 무출력(hang) — 유휴 상한(1s) 초과로 kill되어 result엔 도달하면 안 된다.
    // 경과 시간을 측정해 30s 전체를 기다리지 않고 유휴 상한에서 kill됐음을 증명.
    let script = [
        "printf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s-hang\"}'",
        "sleep 30",
        "printf '%s\\n' '{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done\",\"session_id\":\"s-hang\",\"total_cost_usd\":0.0,\"num_turns\":1}'",
    ]
    .join("\n");
    let (dir, bin) = stub_script(&script);
    let start = std::time::Instant::now();
    // CI 스케줄링으로 첫 printf 자체가 1초를 넘길 수 있어, 초기화 이벤트를 확인하는 이
    // 테스트에는 2초 유휴 상한을 쓴다. 30초 hang 대비 종료 보장은 그대로 검증한다.
    let (res, evs) = run_with_idle_timeout(Vendor::Claude, &bin, 2);
    let elapsed = start.elapsed();
    assert!(
        !evs.iter().any(|e| matches!(e, ConvoEvent::Result { .. })),
        "행(hang) 중 kill이면 Result 이벤트가 없어야 함: {evs:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "30s sleep을 기다리지 않고 유휴 상한에서 kill돼야 함(경과: {elapsed:?})"
    );
    assert_eq!(
        res.as_deref(),
        Ok("s-hang"),
        "init에서 세션은 확보했으므로 Ok(session)이어야 함"
    );
    std::fs::remove_dir_all(&dir).ok();
}
