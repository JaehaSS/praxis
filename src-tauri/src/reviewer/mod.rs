//! 리뷰어 헤드리스 호출 (Framein delegate 이식) — Tauri 비의존.
//!
//! 보안: 리뷰어를 **빈 temp 디렉터리**에서 실행(worktree 아님)해 리뷰어가 툴을 써도
//! worktree를 못 건드리게 한다. 프롬프트는 기본적으로 stdin으로 전달하지만, agy는 CLI 계약상
//! `-p <prompt>` 값으로 전달한다. 타임아웃 시 프로세스 그룹 kill.

use crate::managed_process::SharedProcessRegistrar;

mod process;

/// PATH에서 실행 파일의 **절대 경로**를 찾는다 (간이 which).
/// PTY 스폰(portable_pty)은 프로그램명을 PATH로 해석하지 않으므로 절대경로가 필요하다.
pub fn which(bin: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(bin);
        if p.is_file() {
            return Some(p.to_string_lossy().into_owned());
        }
        #[cfg(windows)]
        {
            for ext in ["cmd", "exe"] {
                let pe = dir.join(format!("{bin}.{ext}"));
                if pe.is_file() {
                    return Some(pe.to_string_lossy().into_owned());
                }
            }
        }
    }
    None
}

/// PATH에 실행 파일이 있는지(간이 which).
pub(crate) fn on_path(bin: &str) -> bool {
    which(bin).is_some()
}

/// 리드와 **다른 벤더**를 우선(교차모델), 없으면 claude 폴백.
pub fn detect_reviewer(lead: &str) -> String {
    for cand in ["codex", "agy", "claude"] {
        if cand != lead && on_path(cand) {
            return cand.to_string();
        }
    }
    "claude".to_string()
}

struct ReviewerInvocation<'a> {
    bin: &'static str,
    args: Vec<String>,
    stdin_prompt: Option<&'a str>,
}

fn invocation<'a>(model: &str, prompt: &'a str) -> ReviewerInvocation<'a> {
    let args = |values: &[&str]| values.iter().map(|value| value.to_string()).collect();
    match model {
        "codex" => ReviewerInvocation {
            bin: "codex",
            args: args(&["exec", "--skip-git-repo-check"]),
            stdin_prompt: Some(prompt),
        },
        // agy 1.1.8의 `-p`는 stdin 스위치가 아니라 프롬프트 값을 요구하는 플래그다.
        "gemini" | "agy" | "antigravity" => ReviewerInvocation {
            bin: "agy",
            args: args(&["-p", prompt]),
            stdin_prompt: None,
        },
        _ => ReviewerInvocation {
            bin: "claude",
            args: args(&["-p", "--output-format", "text"]),
            stdin_prompt: Some(prompt),
        },
    }
}

/// 벤더별 실행 커맨드라인 문자열 생성 (모델 정보 기록용).
pub fn describe_invocation(vendor: &str) -> String {
    let invocation = invocation(vendor, "<prompt>");
    let mut cmd = invocation.bin.to_string();
    for arg in invocation.args {
        cmd.push(' ');
        cmd.push_str(&arg);
    }
    cmd
}

/// 리뷰어를 헤드리스로 실행 → stdout 텍스트. 프롬프트는 stdin, cwd는 빈 temp.
pub fn run_reviewer(model: &str, prompt: &str, timeout_secs: u64) -> Result<String, String> {
    run_reviewer_registered(model, prompt, timeout_secs, None)
}

pub fn run_reviewer_registered(
    model: &str,
    prompt: &str,
    timeout_secs: u64,
    registrar: Option<&SharedProcessRegistrar>,
) -> Result<String, String> {
    let invocation = invocation(model, prompt);
    let args = invocation
        .args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    process::run(
        invocation.bin,
        &args,
        invocation.stdin_prompt.unwrap_or_default(),
        timeout_secs,
        registrar,
    )
}

/// Uses the task's established headless agent invocation in an app-owned candidate.
pub fn run_repair_agent(
    cwd: &std::path::Path, agent: &str, model: Option<&str>, effort: Option<&str>,
    prompt: &str, registrar: &SharedProcessRegistrar,
) -> Result<String, String> {
    if !crate::agent::is_preset(agent) && !matches!(agent, "gemini" | "antigravity") {
        return Err("자동 해결은 등록된 에이전트에서 지원합니다".into());
    }
    let (bin, args) = crate::agent::headless_args_with_effort(agent, prompt, model, effort, None)
        .ok_or_else(|| "에이전트 실행 설정이 없습니다".to_string())?;
    let bin = which(&bin).ok_or_else(|| format!("{bin} 실행 파일을 찾을 수 없습니다"))?;
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    process::run_in_directory(&bin, &args, "", 600, Some(registrar), cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_agy_reserves_print_flag_value_for_prompt() {
        // agy 1.1.8의 `-p`는 stdin 스위치가 아니라 프롬프트 값을 받는 플래그다.
        for vendor in ["agy", "antigravity", "gemini"] {
            let invocation = invocation(vendor, "prompt");
            assert_eq!(invocation.bin, "agy");
            assert_eq!(invocation.args, vec!["-p", "prompt"]);
            assert_eq!(invocation.stdin_prompt, None);
        }
    }

    #[test]
    fn invocation_known_vendors() {
        let codex = invocation("codex", "prompt");
        assert_eq!(codex.bin, "codex");
        assert_eq!(codex.stdin_prompt, Some("prompt"));
        // 미지/기본은 claude 폴백.
        assert_eq!(invocation("claude", "prompt").bin, "claude");
        assert_eq!(invocation("anything-else", "prompt").bin, "claude");
    }
}
