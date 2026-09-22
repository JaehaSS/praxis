//! PATH 보강 — macOS GUI(.app)는 Finder/LaunchServices 실행 시 **최소 PATH**만 받는다
//! (`/usr/bin:/bin:...`). 그러면 claude/codex/gemini(nvm·~/.local/bin), git, 빌드 도구가
//! 셸아웃에서 안 잡힌다. 시작 시 **로그인 셸 PATH를 질의해 병합**해 process env에 설정하면
//! 이후 모든 `Command` spawn이 이를 상속한다 (verify/reviewer/capture/worktree 전부).
//!
//! 병합 로직은 순수(테스트 대상), 셸 질의는 cfg(unix) + 타임아웃.

/// 콜론 구분 경로 조각들을 순서 보존 dedup 병합.
pub fn merge_path(parts: &[&str]) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        for dir in part.split(':') {
            if dir.is_empty() {
                continue;
            }
            if seen.insert(dir.to_string()) {
                out.push(dir.to_string());
            }
        }
    }
    out.join(":")
}

/// 로그인+인터랙티브 셸에서 $PATH를 질의 (nvm 등 .zshrc 설정 포함). 타임아웃 시 None.
#[cfg(unix)]
fn query_login_path() -> Option<String> {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    // 델리미터로 감싸 prompt/clear 등 잡음 출력과 분리.
    let script = r#"printf '__PRAXIS_PATH__%s__PRAXIS_PATH__' "$PATH""#;
    let mut c = Command::new(&shell);
    c.args(["-l", "-i", "-c", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    let child = c.spawn().ok()?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let out = match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(o)) => o,
        _ => {
            crate::verify::kill_group(pid);
            return None;
        }
    };
    let s = String::from_utf8_lossy(&out.stdout);
    let m = "__PRAXIS_PATH__";
    let start = s.find(m)? + m.len();
    let rest = &s[start..];
    let end = rest.find(m)?;
    let path = &rest[..end];
    if path.trim().is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

#[cfg(not(unix))]
fn query_login_path() -> Option<String> {
    None
}

/// 알려진 도구 디렉터리 (로그인 셸 질의 실패 폴백).
fn known_dirs() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
}

/// 시작 시 1회 호출 — 로그인 셸 PATH + 현재 PATH + 알려진 디렉터리를 병합해 설정.
/// macOS/Linux만 (Windows는 GUI도 PATH 정상).
pub fn augment_path() {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let current = std::env::var("PATH").unwrap_or_default();
        let login = query_login_path().unwrap_or_default();
        let known = known_dirs();
        // 우선순위: 로그인 셸(사용자 도구 버전) → 현재 → 알려진 폴백.
        let merged = merge_path(&[&login, &current, &known]);
        if merged != current {
            std::env::set_var("PATH", &merged);
            eprintln!(
                "[praxis] PATH 보강: {} → {} 디렉터리",
                current.split(':').filter(|s| !s.is_empty()).count(),
                merged.split(':').filter(|s| !s.is_empty()).count()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_dedup_preserves_order() {
        assert_eq!(merge_path(&["/a:/b", "/b:/c", "/a"]), "/a:/b:/c");
    }

    #[test]
    fn merge_skips_empty_segments() {
        assert_eq!(merge_path(&["", "/x:", ":/y"]), "/x:/y");
        assert_eq!(merge_path(&[""]), "");
    }

    #[test]
    fn merge_login_takes_precedence() {
        // 로그인 PATH가 앞 → 사용자 도구 디렉터리가 /usr/bin보다 우선.
        let out = merge_path(&["/opt/homebrew/bin:/usr/bin", "/usr/bin:/bin"]);
        assert_eq!(out, "/opt/homebrew/bin:/usr/bin:/bin");
    }
}
