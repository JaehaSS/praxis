//! 벤더 CLI에 직접 물어 인증 상태·버전·설치 방식을 판독한다.
//!
//! 자격증명 파일을 파싱하지 않는 이유: 포맷은 벤더가 예고 없이 바꾸지만 CLI 계약은 안정적이다.
//! `usage::claude_credentials`는 OAuth usage API 호출에 **토큰 값 자체**가 필요해 파일을 읽는다 —
//! 대체가 아니라 병존이고 역할이 갈린다(usage=잔량, agenthealth=인증·버전).
//!
//! 판독 실패는 전부 `Unknown`으로 떨어진다. `LoggedOut`으로 단정하면 멀쩡한 태스크를 차단한다.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;

/// CLI 한 번 호출에 허용하는 시간. 넘으면 Unknown.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// 인증 상태.
///
/// `Expired`를 따로 두지 않는다 — `claude auth status --json`은 `loggedIn` 불리언만 주고
/// 만료와 로그아웃을 구분해주지 않는다. 구분하려면 자격증명 파일을 추가로 읽어야 하는데,
/// **사용자가 할 일은 어느 쪽이든 로그인 하나로 같다.** 구분의 값이 비용보다 작다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    Ok,
    LoggedOut,
    Unknown,
}

/// 설치 방식 — 업데이트 명령이 여기서 파생된다. 하드코딩하면 반드시 틀린다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallMethod {
    Native,
    Npm,
    Homebrew,
    Unknown,
}

/// 실행 파일을 PATH에서 찾는다.
///
/// `reviewer::which`를 쓰지 않는 이유: 그쪽은 확장자 없는 파일을 **먼저** 반환한다.
/// Windows에서 npm 전역 shim은 `codex`(POSIX shell script) · `codex.cmd` · `codex.ps1`
/// 세 벌로 깔리고, 확장자 없는 쪽은 CreateProcess가 실행하지 못한다. 여기서는
/// `.exe` → `.cmd` → 확장자 없음 순으로 뒤져 실행 가능한 것을 고른다.
pub fn resolve_bin(bin: &str) -> Option<String> {
    if Path::new(bin).is_file() {
        return Some(bin.to_string());
    }
    let path = std::env::var("PATH").ok()?;
    #[cfg(windows)]
    let candidates = [format!("{bin}.exe"), format!("{bin}.cmd"), bin.to_string()];
    #[cfg(not(windows))]
    let candidates = [bin.to_string()];
    for dir in std::env::split_paths(&path) {
        for candidate in &candidates {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// 프로세스 하나를 띄워 (성공 여부, stdout+stderr)를 돌려준다. 타임아웃이면 None.
fn probe(bin: &str, args: &[&str]) -> Option<(bool, String)> {
    let mut command = Command::new(bin);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().ok()?;
    let id = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(PROBE_TIMEOUT) {
        Ok(Ok(output)) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            Some((output.status.success(), text))
        }
        // 타임아웃/실패 — 남은 자식은 거둔다. 5초마다 좀비를 쌓을 수는 없다.
        _ => {
            kill_pid(id);
            None
        }
    }
}

#[cfg(unix)]
pub(super) fn kill_pid(pid: u32) {
    let _ = Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(windows)]
pub(super) fn kill_pid(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F", "/T"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

// ── 순수 판독부 (테스트 대상) ─────────────────────────────────────────

/// `claude auth status --json` 출력 → (상태, 계정, 플랜).
///
/// 실제 스키마: `{ loggedIn, authMethod, apiProvider, email, orgId, orgName, subscriptionType }`.
pub fn parse_claude_auth(raw: &str) -> (AuthState, Option<String>, Option<String>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return (AuthState::Unknown, None, None);
    };
    let Some(logged_in) = value.get("loggedIn").and_then(serde_json::Value::as_bool) else {
        // 스키마가 바뀌었다 — 로그아웃으로 단정하지 않는다.
        return (AuthState::Unknown, None, None);
    };
    let account = value
        .get("email")
        .and_then(serde_json::Value::as_str)
        .filter(|email| !email.is_empty())
        .map(str::to_string);
    let plan = value
        .get("subscriptionType")
        .and_then(serde_json::Value::as_str)
        .filter(|plan| !plan.is_empty())
        .map(str::to_string);
    let state = if logged_in {
        AuthState::Ok
    } else {
        AuthState::LoggedOut
    };
    (state, account, plan)
}

/// `codex login status` 결과 → (상태, 부연).
///
/// JSON을 주지 않으므로 exit code가 유일한 신호다. stdout(`Logged in using ChatGPT`)은
/// 툴팁용 부연으로 그대로 흘린다.
pub fn parse_codex_auth(success: bool, output: &str) -> (AuthState, Option<String>) {
    let detail = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string);
    let state = if success {
        AuthState::Ok
    } else {
        AuthState::LoggedOut
    };
    (state, detail)
}

/// 실행 파일 경로 → 설치 방식.
///
/// 구분자를 `/`로 정규화해 Windows 경로도 같은 규칙으로 본다.
pub fn install_method_of(path: &str) -> InstallMethod {
    let normalized = path.replace('\\', "/").to_lowercase();
    // Homebrew를 먼저 — Cellar 안에 node_modules가 섞여 있어도 brew가 소유자다.
    if normalized.contains("/homebrew/")
        || normalized.contains("/cellar/")
        || normalized.contains("linuxbrew")
    {
        return InstallMethod::Homebrew;
    }
    if normalized.contains("node_modules")
        || normalized.contains("/npm/")
        || normalized.contains("/.nvm/")
        || normalized.contains("/fnm/")
    {
        return InstallMethod::Npm;
    }
    if normalized.contains("/.local/bin/") || normalized.contains("/.claude/local/") {
        return InstallMethod::Native;
    }
    InstallMethod::Unknown
}

/// `--version` 출력에서 첫 dotted 숫자 토큰을 뽑는다.
/// `codex-cli 0.111.0` · `2.1.241 (Claude Code)` 둘 다 같은 규칙으로 걸린다.
pub fn parse_version(raw: &str) -> Option<String> {
    for token in raw.split(|c: char| c.is_whitespace() || c == '(' || c == ')') {
        let trimmed = token.trim_matches(|c: char| !c.is_ascii_digit());
        if trimmed.contains('.') && trimmed.split('.').all(|part| !part.is_empty()) {
            let numeric = trimmed
                .split('.')
                .all(|part| part.chars().all(|c| c.is_ascii_digit()));
            if numeric && trimmed.split('.').count() >= 2 {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// `latest`가 `installed`보다 새로운가. 판독 불가면 false — 없는 업데이트를 권하지 않는다.
pub fn is_newer(installed: &str, latest: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        // `"".split('.')`는 빈 vec이 아니라 `[""]`를 낸다 — 세그먼트가 숫자로 시작하는지
        // 먼저 보지 않으면 빈 문자열이 `[0]`으로 통과해 없는 업데이트를 권하게 된다.
        let segments: Vec<&str> = v.split('.').collect();
        if segments
            .iter()
            .any(|part| !part.starts_with(|c: char| c.is_ascii_digit()))
        {
            return Vec::new();
        }
        segments
            .iter()
            .map(|part| {
                let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
                digits.parse::<u64>().unwrap_or(0)
            })
            .collect()
    };
    let (a, b) = (parse(installed), parse(latest));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        if right != left {
            return right > left;
        }
    }
    false
}

// ── CLI 호출부 ───────────────────────────────────────────────────────

/// 설치 버전 — 실행 파일이 없거나 응답이 없으면 None.
pub fn installed_version(bin: &str) -> Option<String> {
    let path = resolve_bin(bin)?;
    let (_, output) = probe(&path, &["--version"])?;
    parse_version(&output)
}

/// 인증 상태 조회. 실행 파일이 없으면 `Unknown`.
pub fn auth_of(vendor: &str) -> (AuthState, Option<String>, Option<String>) {
    match vendor {
        "claude" => match resolve_bin("claude")
            .and_then(|path| probe(&path, &["auth", "status", "--json"]).map(|(_, output)| output))
        {
            Some(output) => parse_claude_auth(&output),
            None => (AuthState::Unknown, None, None),
        },
        "codex" => match resolve_bin("codex").and_then(|path| probe(&path, &["login", "status"])) {
            Some((success, output)) => {
                let (state, detail) = parse_codex_auth(success, &output);
                (state, None, detail)
            }
            None => (AuthState::Unknown, None, None),
        },
        _ => (AuthState::Unknown, None, None),
    }
}

/// 설치 방식 — 실행 파일을 못 찾으면 `Unknown`(버튼이 비활성화된다).
pub fn install_method_of_bin(bin: &str) -> InstallMethod {
    resolve_bin(bin).map_or(InstallMethod::Unknown, |path| install_method_of(&path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_auth_reads_real_schema() {
        // `claude auth status --json` 2.1.241 실제 출력.
        let raw = r#"{
          "loggedIn": true,
          "authMethod": "claude.ai",
          "apiProvider": "firstParty",
          "email": "user@example.com",
          "orgId": "195172e3-c047-449a-9f25-2a95ff72d94b",
          "orgName": "user@example.com's Organization",
          "subscriptionType": "max"
        }"#;
        let (state, account, plan) = parse_claude_auth(raw);
        assert_eq!(state, AuthState::Ok);
        assert_eq!(account.as_deref(), Some("user@example.com"));
        assert_eq!(plan.as_deref(), Some("max"));
    }

    #[test]
    fn claude_logged_out_is_logged_out() {
        let (state, account, _) = parse_claude_auth(r#"{"loggedIn": false}"#);
        assert_eq!(state, AuthState::LoggedOut);
        assert!(account.is_none());
    }

    #[test]
    fn claude_schema_change_yields_unknown_not_logged_out() {
        // loggedIn이 사라지면 알 수 없는 것이지 로그아웃이 아니다 — 태스크를 잘못 막지 않는다.
        assert_eq!(
            parse_claude_auth(r#"{"authenticated": true}"#).0,
            AuthState::Unknown
        );
        assert_eq!(parse_claude_auth("not json at all").0, AuthState::Unknown);
    }

    #[test]
    fn codex_auth_follows_exit_code_and_keeps_first_line() {
        let (state, detail) = parse_codex_auth(true, "Logged in using ChatGPT\n");
        assert_eq!(state, AuthState::Ok);
        assert_eq!(detail.as_deref(), Some("Logged in using ChatGPT"));

        let (state, _) = parse_codex_auth(false, "Not logged in\n");
        assert_eq!(state, AuthState::LoggedOut);
    }

    #[test]
    fn install_method_reads_observed_paths() {
        // 이 개발기에서 실제로 관측된 두 경로.
        assert_eq!(
            install_method_of("C:\\Users\\me\\AppData\\Roaming\\npm\\codex.cmd"),
            InstallMethod::Npm
        );
        assert_eq!(
            install_method_of("C:\\Users\\me\\.local\\bin\\claude.exe"),
            InstallMethod::Native
        );
        assert_eq!(
            install_method_of("/opt/homebrew/bin/codex"),
            InstallMethod::Homebrew
        );
        assert_eq!(
            install_method_of("/usr/lib/node_modules/.bin/codex"),
            InstallMethod::Npm
        );
        assert_eq!(
            install_method_of("/home/me/.claude/local/claude"),
            InstallMethod::Native
        );
        assert_eq!(
            install_method_of("/opt/weird/codex"),
            InstallMethod::Unknown
        );
    }

    #[test]
    fn homebrew_wins_over_node_modules_inside_cellar() {
        assert_eq!(
            install_method_of("/opt/homebrew/Cellar/codex/0.1/libexec/node_modules/.bin/codex"),
            InstallMethod::Homebrew
        );
    }

    #[test]
    fn version_parses_both_vendor_formats() {
        assert_eq!(
            parse_version("codex-cli 0.111.0").as_deref(),
            Some("0.111.0")
        );
        assert_eq!(
            parse_version("2.1.241 (Claude Code)").as_deref(),
            Some("2.1.241")
        );
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn observed_codex_lag_is_detected() {
        // 설계 착수 시점의 실측: 설치 0.111.0 · npm 최신 0.149.0.
        assert!(is_newer("0.111.0", "0.149.0"));
        assert!(!is_newer("2.1.241", "2.1.241"));
        assert!(!is_newer("0.149.0", "0.111.0"));
    }

    #[test]
    fn version_compare_is_numeric_not_lexical() {
        // 문자열 비교였다면 "0.9.0" > "0.111.0"이 되어 업데이트를 놓친다.
        assert!(is_newer("0.9.0", "0.111.0"));
        assert!(!is_newer("0.111.0", "0.9.0"));
    }

    #[test]
    fn unparseable_versions_never_claim_an_update() {
        assert!(!is_newer("", "1.0.0"));
        assert!(!is_newer("1.0.0", ""));
        assert!(!is_newer("unknown", "1.0.0"));
        // 프리릴리스 꼬리는 숫자로 시작하므로 유효 — 0.111.0 → 0.149.0-rc.1은 업데이트다.
        assert!(is_newer("0.111.0", "0.149.0-rc.1"));
    }
}
