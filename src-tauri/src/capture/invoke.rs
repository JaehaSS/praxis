//! 캡처·회고의 **린 인보케이션** — 설계 0055.
//!
//! 이 모듈이 존재하는 이유는 비용이 아니라 **명시**다. 플래그 없는 `claude -p`는
//! `~/.claude/settings.json`의 모델·effort를 조용히 상속하고, 도구 31종·MCP·스킬 열거를
//! 매 호출의 시스템 프롬프트에 싣는다. 한 문장 요약에 호출당 약 28,600토큰의 하네스
//! 오버헤드가 붙었고 그것이 비용의 79%였다(브리프 §2.2).
//!
//! 세 가지를 함께 고친다.
//! - **명시적 실행 프로파일** — 미설정·무효는 CLI 기본이 아니라 코드 상수로 접는다(AD-4).
//!   명시하지 않은 것은 언젠가 사용자의 최상위 모델이 된다.
//! - **신뢰 경계** — 트랜스크립트는 툴 결과·레포 파일을 담은 신뢰할 수 없는 입력이다.
//!   `--tools ""`가 도구 표면을 0으로 만들어, 프롬프트 본문의 가드 문구를 유일한 방어에서
//!   2차 방어로 강등한다(AD-3).
//! - **관측** — 근본 원인은 모델 상속이 아니라 관측 부재였다. 무엇이 어떤 모델로 돌았고
//!   파싱에 성공했는지를 남긴다(AD-7).
//!
//! 구조는 `reviewer::invocation`을 본뜬다 — 인자 조립을 순수 함수로 갈라 프로세스 없이
//! 어서션한다. 이 저장소는 인자 한 글자로 무출력 즉사한 적이 있다(agy `-m`).

use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use sqlx::SqlitePool;

/// 두 호출 공용 시스템 프롬프트. 역할 서술은 이미 프롬프트 본문에 있으므로 최소로 둔다 —
/// 둘로 쪼개면 같은 말이 네 곳에 흩어진다.
pub(crate) const CAPTURE_SYSTEM_PROMPT: &str =
    "너는 텍스트 분석 도구다. 도구를 호출하지 않고, 요청된 형식의 텍스트만 출력한다.";

/// 설정이 비어 있을 때 떨어질 자리. **CLI 기본값으로 두지 않는다** — 그것이 이 버그였다.
pub(crate) const DEFAULT_MODEL: &str = "sonnet";
pub(crate) const DEFAULT_EFFORT: &str = "low";

pub(crate) const KEY_MODEL: &str = "capture:model";
pub(crate) const KEY_EFFORT: &str = "capture:effort";
pub(crate) const KEY_LEAN: &str = "capture:lean";

/// 호출의 종류. **관측 슬롯을 가르는 키이기도 하다** — 한 작업에서 추출 다음에 회고가
/// 돌므로, 슬롯이 하나면 회고가 추출의 기록을 언제나 덮는다(설계 §9).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureKind {
    Extract,
    Reflect,
}

impl CaptureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CaptureKind::Extract => "extract",
            CaptureKind::Reflect => "reflect",
        }
    }
}

/// 이번 호출에 적용할 실행 프로파일.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CaptureProfile {
    pub model: String,
    pub effort: String,
    /// false면 §12 롤백 집합대로 되돌린다. 시스템 프롬프트·출력 형식·설정 소스만 되돌리고
    /// 모델·effort·도구 차단은 유지한다.
    pub lean: bool,
}

impl Default for CaptureProfile {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            effort: DEFAULT_EFFORT.to_string(),
            lean: true,
        }
    }
}

/// 한 번의 호출이 남기는 관측 기록. 비용만으로는 "0원 = 안 돌았다"와 "0원 = 껐다"가
/// 구분되지 않으므로 `ok`·`parsed_ok`·`err`를 함께 든다.
#[derive(Clone, Debug, Serialize)]
pub struct CaptureRun {
    pub model: String,
    pub effort: String,
    pub lean: bool,
    /// `lean=false`(text 출력)에서는 봉투가 없어 회수할 수 없다 — `None`을 "0원"으로 읽히게
    /// 두지 말고 UI가 "비용 미측정"으로 표시해야 한다.
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// 프로세스 exit + 봉투 `is_error`를 합친 최종 성공 여부.
    pub ok: bool,
    /// 호출자가 기대한 구조(JSON 배열/문장)를 실제로 얻었는지. 조용한 0건 캡처의 탐지기다.
    pub parsed_ok: bool,
    /// 인용 판정(ride)을 요청한 호출에서만 의미가 있다. `None`이면 요청하지 않은 것.
    pub citation_found: Option<bool>,
    /// 실패 이유. stderr는 원래 폐기됐다 — 이유 없는 실패 표시는 관측을 한 칸 옮길 뿐이다.
    pub err: Option<String>,
    pub at: i64,
}

/// 마지막 실행 기록 — `kind`별로 하나씩. 슬롯을 가르지 않으면 회고가 추출을 덮는다.
static LAST_RUNS: OnceLock<Mutex<HashMap<CaptureKind, CaptureRun>>> = OnceLock::new();

fn slots() -> &'static Mutex<HashMap<CaptureKind, CaptureRun>> {
    LAST_RUNS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 락 poisoning에서 관측을 포기하지 않는다 — 여기 담긴 것은 불변식 없는 순수 관측 데이터다.
/// `if let Ok(..)`로 두면 한 번 오염된 뒤 모든 기록이 조용히 버려지고, 관측 부재가 근본
/// 원인이라던 설계가 자기 관측을 스스로 끄게 된다.
pub(crate) fn record(kind: CaptureKind, run: CaptureRun) {
    let mut m = slots().lock().unwrap_or_else(|e| e.into_inner());
    m.insert(kind, run);
}

/// 설정 화면용 조회 — `kind` 문자열 키로 낸다.
pub fn last_runs() -> HashMap<String, CaptureRun> {
    let m = slots().lock().unwrap_or_else(|e| e.into_inner());
    m.iter()
        .map(|(k, v)| (k.as_str().to_string(), v.clone()))
        .collect()
}

#[cfg(test)]
pub(crate) fn clear_runs() {
    slots().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// 설정에서 프로파일 해석. 미설정·빈값·무효는 전부 코드 상수로 접는다(AD-4).
///
/// effort는 `reasoning_effort_override`로 검증한다 — 무효값을 그대로 넘기면 CLI가 즉사하고,
/// 그 실패는 `Ok(None)`으로 삼켜져 조용한 0건이 된다.
pub(crate) async fn profile(pool: &SqlitePool) -> CaptureProfile {
    let get = |key: &'static str| async move {
        crate::db::get_setting(pool, key)
            .await
            .ok()
            .flatten()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let model = get(KEY_MODEL).await.unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let effort = get(KEY_EFFORT)
        .await
        .and_then(|e| {
            crate::agent::reasoning_effort_override("claude", Some(&e))
                .ok()
                .flatten()
        })
        .unwrap_or_else(|| DEFAULT_EFFORT.to_string());
    // 미설정은 린(기본 on). 명시적으로 "false"일 때만 롤백한다.
    let lean = get(KEY_LEAN).await.as_deref() != Some("false");
    CaptureProfile {
        model,
        effort,
        lean,
    }
}

/// Consent and queued work bind to every profile field that affects an invocation.
pub(crate) fn provider_identity(profile: &CaptureProfile) -> String {
    format!(
        "claude:{}:{}:{}",
        profile.model,
        profile.effort,
        if profile.lean { "lean" } else { "standard" }
    )
}

pub(crate) fn provider_display(profile: &CaptureProfile) -> String {
    format!(
        "Claude · {} · {}{}",
        profile.model,
        profile.effort,
        if profile.lean { " · lean" } else { "" }
    )
}

/// 인자 벡터 조립 — **순수 함수**. 프로세스를 띄우지 않고 어서션할 수 있어야 한다.
///
/// 순서가 계약이다. `-p`는 `--print` 불린이고 프롬프트는 위치 인자이며 `--tools`는 variadic
/// (`<tools...>`)이다. 프롬프트를 `--tools` 뒤에 두면 그 값으로 먹힌다.
pub(crate) fn invocation_args(profile: &CaptureProfile, prompt: &str) -> Vec<String> {
    let s = |v: &str| v.to_string();
    let mut args = vec![s("-p"), s(prompt)];

    args.extend([s("--output-format"), s(if profile.lean { "json" } else { "text" })]);
    // 모델·effort는 롤백 대상이 아니다 — 되돌리면 상속이 살아나 이 작업의 존재 이유가 사라진다.
    args.extend([s("--model"), s(&profile.model)]);
    args.extend([s("--effort"), s(&profile.effort)]);
    if profile.lean {
        args.extend([s("--system-prompt"), s(CAPTURE_SYSTEM_PROMPT)]);
        // user/project/local 설정을 건너뛴다. managed 설정과 인증은 그대로 적용된다.
        // 롤백 시 이 플래그가 빠지는 것이 `apiKeyHelper` 환경의 유일한 구제 수단이다.
        args.extend([s("--setting-sources"), s("")]);
    }
    // 신뢰 경계와 파일 위생은 롤백해도 유지한다.
    args.extend([s("--tools"), s("")]);
    args.push(s("--strict-mcp-config"));
    args.push(s("--no-session-persistence"));
    args
}

/// 봉투에서 뽑아낸 것. 호출자에게는 `text`만 나간다 — 봉투 원문은 이 모듈 밖으로 안 나간다.
#[derive(Debug, PartialEq)]
pub(crate) struct Envelope {
    pub text: String,
    pub is_error: bool,
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub diagnostic: Option<String>,
}

/// `--output-format json` 봉투 파싱.
///
/// **`subtype`으로 실패를 판정하지 않는다.** 실측(CLI 2.1.258)에서 잘못된 모델명은
/// `is_error: true`이면서 `subtype: "success"`인 봉투를 냈다. `subtype`만 보면 이 실패를
/// 놓치고, 사람이 읽는 오류 문장이 그대로 회고로 저장된다.
pub(crate) fn parse_envelope(stdout: &str) -> Option<Envelope> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    let text = v.get("result")?.as_str()?.to_string();
    let usage = v.get("usage");
    let tok = |k: &str| usage.and_then(|u| u.get(k)).and_then(|x| x.as_u64());
    let diagnostic = v
        .get("terminal_reason")
        .and_then(|x| x.as_str())
        .map(|s| {
            match v.get("api_error_status").and_then(|x| x.as_u64()) {
                Some(code) => format!("{s} ({code})"),
                None => s.to_string(),
            }
        });
    Some(Envelope {
        text,
        is_error: v.get("is_error").and_then(|x| x.as_bool()).unwrap_or(false),
        cost_usd: v.get("total_cost_usd").and_then(|x| x.as_f64()),
        input_tokens: tok("input_tokens"),
        output_tokens: tok("output_tokens"),
        diagnostic,
    })
}

/// stderr 꼬리 — 진단에 쓸 만큼만. `[claude-code:unrecognized_model] {...}` 같은 한 줄이 온다.
fn stderr_tail(bytes: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(bytes);
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    Some(t.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect())
}

/// 린 인보케이션 실행. 실패는 전부 `None`으로 접되 **이유를 기록에 남긴다.**
///
/// 판정 순서는 exit code → `is_error` → 봉투 파싱이며, 셋 중 하나라도 실패면 접는다.
/// `parsed_ok`는 호출자가 나중에 `mark_parsed`로 갱신한다 — 여기서는 텍스트를 얻었는지까지만 안다.
pub(crate) fn run(profile: &CaptureProfile, kind: CaptureKind, prompt: &str) -> Option<String> {
    let args = invocation_args(profile, prompt);
    let mut base = CaptureRun {
        model: profile.model.clone(),
        effort: profile.effort.clone(),
        lean: profile.lean,
        cost_usd: None,
        input_tokens: None,
        output_tokens: None,
        ok: false,
        parsed_ok: false,
        citation_found: None,
        err: None,
        at: crate::now(),
    };

    let out = match Command::new("claude").args(&args).output() {
        Ok(o) => o,
        Err(e) => {
            base.err = Some(format!("claude 실행 실패: {e}"));
            record(kind, base);
            return None;
        }
    };

    if !out.status.success() {
        base.err = stderr_tail(&out.stderr)
            .or_else(|| Some(format!("exit {}", out.status.code().unwrap_or(-1))));
        record(kind, base);
        return None;
    }

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !profile.lean {
        // 롤백 경로 — 봉투가 없으므로 비용은 회수할 수 없다.
        base.ok = true;
        record(kind, base);
        return Some(stdout);
    }

    let Some(env) = parse_envelope(&stdout) else {
        // 계약 4 — 이유를 버리지 않는다. 여기서 받은 것이 무엇이었는지가 유일한 단서다
        // (exit는 0이었으므로 stderr가 비어 있을 수 있다).
        let head: String = stdout.trim().chars().take(200).collect();
        base.err = Some(format!(
            "봉투 파싱 실패 — --output-format json 응답이 아님. 받은 것: {}",
            if head.is_empty() {
                stderr_tail(&out.stderr).unwrap_or_else(|| "(빈 출력)".to_string())
            } else {
                head
            }
        ));
        record(kind, base);
        return None;
    };
    base.cost_usd = env.cost_usd;
    base.input_tokens = env.input_tokens;
    base.output_tokens = env.output_tokens;
    if env.is_error {
        // 오류 문장이 회고가 되지 않도록 여기서 접는다.
        base.err = env.diagnostic.or_else(|| Some(env.text.chars().take(200).collect()));
        record(kind, base);
        return None;
    }
    base.ok = true;
    record(kind, base);
    Some(env.text)
}

/// 호출자가 산출물 파싱 결과를 알린 뒤 기록을 갱신한다. 조용한 0건의 유일한 탐지기다.
pub(crate) fn mark_parsed(kind: CaptureKind, parsed_ok: bool, citation_found: Option<bool>) {
    let mut m = slots().lock().unwrap_or_else(|e| e.into_inner());
    // `run()`의 모든 return 지점이 `record`를 선행하므로 슬롯은 반드시 존재한다.
    // 없으면 호출 순서가 깨진 것이고, 조용한 no-op로 두면 그 사실이 보이지 않는다.
    debug_assert!(m.contains_key(&kind), "mark_parsed는 record 이후에만 호출된다");
    if let Some(r) = m.get_mut(&kind) {
        r.parsed_ok = parsed_ok;
        r.citation_found = citation_found;
    }
}

