//! Python REPL(IPython) 런처 해석 — Tauri 비의존. `cargo test`로 직접 검증 가능.
//!
//! `commands/repl.rs`가 이 모듈로 "무엇을 스폰할지"를 물어보고, 그 결과로 PTY만 띄운다.
//! PATH 탐색 로직을 순수 함수로 분리해 둔 이유는 GUI 앱의 PATH가 로그인 셸의 PATH와
//! 다르기 때문이다(pyenv shim, Homebrew, framework 빌드 등이 로그인 셸 rc에서만 주입된다).

use std::path::Path;
use std::process::Command;

/// `resolve()`가 내놓는 판정 — 바로 스폰 가능한 ipython, 또는 없어서 설치가 필요한 상태.
pub enum ReplLaunch {
    /// 바로 스폰할 수 있는 ipython.
    Ipython { cmd: String, args: Vec<String> },
    /// ipython이 없다. `python`은 대신 쓸 수 있는 python3 절대경로(있으면) — 설치 스크립트를
    /// 그 인터프리터로 돌리는 데 쓴다. 그마저 없으면 `None`.
    Missing { python: Option<String> },
}

#[cfg(windows)]
const IPYTHON_RELATIVE: &str = "Scripts\\ipython.exe";
#[cfg(not(windows))]
const IPYTHON_RELATIVE: &str = "bin/ipython";

/// 실제 환경에서 쓰는 진입점 — PATH 탐색은 로그인 셸을 통해서 한다(`login_shell_which`).
pub fn resolve(root: &Path) -> ReplLaunch {
    let virtual_env = std::env::var("VIRTUAL_ENV").ok();
    resolve_with(root, virtual_env.as_deref(), &|bin| login_shell_which(bin))
}

/// `resolve`의 순수 버전 — PATH 탐색을 주입받아 테스트가 실제 PATH에 기대지 않게 한다.
///
/// 우선순위: `$VIRTUAL_ENV/bin/ipython` → `root/.venv/bin/ipython` → `root/venv/bin/ipython`
/// → PATH의 `ipython` → (없으면) PATH의 `python3`를 실은 `Missing`.
pub fn resolve_with(
    root: &Path,
    virtual_env: Option<&str>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> ReplLaunch {
    if let Some(venv) = virtual_env {
        let candidate = Path::new(venv).join(IPYTHON_RELATIVE);
        if candidate.is_file() {
            return ipython_at(&candidate);
        }
    }
    for venv_dir in [".venv", "venv"] {
        let candidate = root.join(venv_dir).join(IPYTHON_RELATIVE);
        if candidate.is_file() {
            return ipython_at(&candidate);
        }
    }
    if let Some(path) = lookup("ipython") {
        return ReplLaunch::Ipython {
            cmd: path,
            args: Vec::new(),
        };
    }
    ReplLaunch::Missing {
        python: lookup("python3"),
    }
}

fn ipython_at(path: &Path) -> ReplLaunch {
    ReplLaunch::Ipython {
        cmd: path.to_string_lossy().into_owned(),
        args: Vec::new(),
    }
}

/// 로그인 셸(`$SHELL`, 기본 `/bin/zsh`)의 PATH로 `bin`을 찾는다.
///
/// GUI로 뜬 Tauri 프로세스는 launchd가 물려준 최소 PATH만 보므로, pyenv shim이나
/// `/Library/Frameworks/Python.framework/.../bin`처럼 로그인 셸 rc 파일에서만 PATH에
/// 얹히는 경로를 놓친다. `-lc "command -v <bin>"`으로 로그인 셸이 직접 찾게 시킨 뒤,
/// 그마저 실패하면 GUI 프로세스 자신의 PATH로 한 번 더(`reviewer::which`) 시도한다.
pub fn login_shell_which(bin: &str) -> Option<String> {
    try_login_shell_which(bin).or_else(|| crate::reviewer::which(bin))
}

fn try_login_shell_which(bin: &str) -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let output = Command::new(&shell)
        .arg("-lc")
        .arg(format!("command -v {bin}"))
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let candidate = stdout.lines().last()?.trim();
    let path = Path::new(candidate);
    (path.is_absolute() && path.is_file()).then(|| candidate.to_string())
}

/// IPython bracketed-paste 페이로드로 감싼다.
///
/// IPython 9 / prompt_toolkit 3에서 실측한 결과 — paste 안에 후행 개행을 넣고 그 뒤에
/// CR을 하나 더 보내야 여러 줄 블록이 그 자리에서 실행된다. 후행 개행이 없으면 블록을
/// "계속 입력 중"으로만 인식하고 실행하지 않는다.
pub fn bracketed_paste(code: &str) -> String {
    format!("\x1b[200~{}\n\x1b[201~\r", code.trim_end())
}

/// IPython이 없을 때 설치와 기동을 한 프로세스 안에서 잇는 스크립트.
///
/// `python -c`로 넘긴다 — 별도 스크립트 파일을 워크트리나 임시 디렉터리에 남기지 않는다.
/// 설치가 끝나면 같은 인터프리터 안에서 바로 `IPython.start_ipython`을 호출해, 사용자가
/// "설치 → 재실행"을 한 번 더 요청하지 않아도 되게 한다.
pub const INSTALL_SCRIPT: &str = r#"import subprocess, sys
args = [sys.executable, "-m", "pip", "install", "ipython"]
if sys.prefix == getattr(sys, "base_prefix", sys.prefix):
    args.append("--user")
print("IPython을 설치합니다:", " ".join(args), flush=True)
subprocess.check_call(args)
import IPython
sys.exit(IPython.start_ipython(argv=[]))
"#;

/// `python -c <INSTALL_SCRIPT>`로 스폰할 (cmd, args).
pub fn install_launch(python: &str) -> (String, Vec<String>) {
    (
        python.to_string(),
        vec!["-c".to_string(), INSTALL_SCRIPT.to_string()],
    )
}

/// IPython 프롬프트(`In [`) 탐지기 — 청크 경계에 걸쳐도 놓치지 않는다.
///
/// PTY 출력은 임의 크기로 쪼개져 도착하므로 `"In"`과 `" [1]: "`가 서로 다른 `feed` 호출에
/// 나뉘어 올 수 있다. 매 호출 끝에 패턴 길이보다 한 바이트 짧은 tail을 남겨 다음 호출
/// 앞에 붙이는 방식으로, 어느 경계에서 잘려도 놓치지 않는다.
pub struct PromptScanner {
    tail: Vec<u8>,
}

const PROMPT_PATTERN: &[u8] = b"In [";

impl Default for PromptScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptScanner {
    pub fn new() -> Self {
        Self { tail: Vec::new() }
    }

    /// 청크를 먹인다. 이번 호출(이전 tail 포함)에서 프롬프트 패턴을 처음 봤으면 `true`.
    pub fn feed(&mut self, chunk: &[u8]) -> bool {
        let mut combined = std::mem::take(&mut self.tail);
        combined.extend_from_slice(chunk);
        let found = combined
            .windows(PROMPT_PATTERN.len())
            .any(|w| w == PROMPT_PATTERN);
        let keep = PROMPT_PATTERN.len().saturating_sub(1);
        let start = combined.len().saturating_sub(keep);
        self.tail = combined[start..].to_vec();
        found
    }
}

#[cfg(test)]
mod 런처_해석_tests {
    use super::*;

    fn 항상_없음(_bin: &str) -> Option<String> {
        None
    }

    #[test]
    fn venv도_path도_없으면_python3를_찾아_missing을_돌려준다() {
        let root = crate::testtmp::dir().join("repl_launch_none");
        std::fs::create_dir_all(&root).unwrap();
        let lookup = |bin: &str| (bin == "python3").then(|| "/usr/bin/python3".to_string());
        match resolve_with(&root, None, &lookup) {
            ReplLaunch::Missing { python } => {
                assert_eq!(python.as_deref(), Some("/usr/bin/python3"))
            }
            ReplLaunch::Ipython { .. } => panic!("ipython이 없어야 함"),
        }
    }

    #[test]
    fn 아무것도_없으면_missing_python_none() {
        let root = crate::testtmp::dir().join("repl_launch_nothing");
        std::fs::create_dir_all(&root).unwrap();
        match resolve_with(&root, None, &항상_없음) {
            ReplLaunch::Missing { python } => assert!(python.is_none()),
            ReplLaunch::Ipython { .. } => panic!("ipython이 없어야 함"),
        }
    }

    #[test]
    fn path의_ipython을_쓴다_venv가_없으면() {
        let root = crate::testtmp::dir().join("repl_launch_path_only");
        std::fs::create_dir_all(&root).unwrap();
        let lookup = |bin: &str| (bin == "ipython").then(|| "/usr/local/bin/ipython".to_string());
        match resolve_with(&root, None, &lookup) {
            ReplLaunch::Ipython { cmd, args } => {
                assert_eq!(cmd, "/usr/local/bin/ipython");
                assert!(args.is_empty());
            }
            ReplLaunch::Missing { .. } => panic!("PATH의 ipython을 써야 함"),
        }
    }

    #[test]
    fn dot_venv가_path보다_우선한다() {
        let root = crate::testtmp::dir().join("repl_launch_dotvenv");
        let venv_bin = root.join(".venv/bin");
        std::fs::create_dir_all(&venv_bin).unwrap();
        std::fs::write(venv_bin.join("ipython"), b"").unwrap();
        let lookup = |bin: &str| (bin == "ipython").then(|| "/usr/local/bin/ipython".to_string());
        match resolve_with(&root, None, &lookup) {
            ReplLaunch::Ipython { cmd, .. } => {
                assert_eq!(cmd, venv_bin.join("ipython").to_string_lossy());
            }
            ReplLaunch::Missing { .. } => panic!(".venv가 있으면 그걸 써야 함"),
        }
    }

    #[test]
    fn venv_디렉터리도_dot_venv가_없으면_시도한다() {
        let root = crate::testtmp::dir().join("repl_launch_venv_plain");
        let venv_bin = root.join("venv/bin");
        std::fs::create_dir_all(&venv_bin).unwrap();
        std::fs::write(venv_bin.join("ipython"), b"").unwrap();
        match resolve_with(&root, None, &항상_없음) {
            ReplLaunch::Ipython { cmd, .. } => {
                assert_eq!(cmd, venv_bin.join("ipython").to_string_lossy());
            }
            ReplLaunch::Missing { .. } => panic!("venv/bin이 있으면 그걸 써야 함"),
        }
    }

    #[test]
    fn virtual_env가_dot_venv보다_우선한다() {
        let root = crate::testtmp::dir().join("repl_launch_virtualenv_wins");
        let dot_venv_bin = root.join(".venv/bin");
        std::fs::create_dir_all(&dot_venv_bin).unwrap();
        std::fs::write(dot_venv_bin.join("ipython"), b"").unwrap();

        let active = crate::testtmp::dir().join("repl_launch_active_venv");
        let active_bin = active.join("bin");
        std::fs::create_dir_all(&active_bin).unwrap();
        std::fs::write(active_bin.join("ipython"), b"").unwrap();

        match resolve_with(&root, Some(active.to_str().unwrap()), &항상_없음) {
            ReplLaunch::Ipython { cmd, .. } => {
                assert_eq!(cmd, active_bin.join("ipython").to_string_lossy());
            }
            ReplLaunch::Missing { .. } => panic!("VIRTUAL_ENV가 있으면 그걸 써야 함"),
        }
    }

    #[test]
    fn bracketed_paste_한줄() {
        assert_eq!(bracketed_paste("1+1"), "\x1b[200~1+1\n\x1b[201~\r");
    }

    #[test]
    fn bracketed_paste_여러줄과_후행_공백줄은_잘려나간다() {
        let code = "def f():\n    return 1\n\n\n";
        assert_eq!(
            bracketed_paste(code),
            "\x1b[200~def f():\n    return 1\n\x1b[201~\r"
        );
    }

    #[test]
    fn 프롬프트가_한_청크에_있으면_감지한다() {
        let mut scanner = PromptScanner::new();
        assert!(scanner.feed(b"...\nIn [1]: "));
    }

    #[test]
    fn 프롬프트가_청크_경계에_걸쳐도_감지한다() {
        let mut scanner = PromptScanner::new();
        assert!(!scanner.feed(b"...\nI"));
        assert!(scanner.feed(b"n [1]: "));
    }

    #[test]
    fn 무관한_출력에는_반응하지_않는다() {
        let mut scanner = PromptScanner::new();
        assert!(!scanner.feed(b"hello world\n"));
        assert!(!scanner.feed(b"Installing In fooBar\n")); // "In "만 있고 "["가 없다
    }
}
