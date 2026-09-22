//! 에디터가 직접 띄우는 **raw LSP 서버** 스펙 — 파일 확장자로 고른다.
//!
//! `lspdetect`와 목적이 다르다. 저쪽은 에이전트에게 물릴 `lsp-mcp` 브리지를 `.mcp.json`에
//! 자동 주입하는 쪽이고(워크트리 단위·npx 경유), 여기는 IDE 에디터가 stdio로 직접 말을 거는
//! 서버다. 브리지를 한 겹 끼우면 점프 한 번에 npx 부팅이 붙어 대화형으로 못 쓴다.

use std::path::Path;

/// 서버가 "의미 분석이 끝났다"를 알리는 방식. 전부 **에지 트리거 알림**이므로
/// 받는 쪽은 최신값 저장이 아니라 래치여야 한다(설계 0065 DR-2b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessSignal {
    /// rust-analyzer — `experimental/serverStatus`.
    ServerStatus,
    /// jdtls — `language/status`의 `ServiceReady`.
    LanguageStatus,
    /// typescript-language-server — 워크스페이스 진행 토큰의 `end`.
    Progress,
    /// pyright — 기다릴 신호가 없다.
    Unsupported,
}

/// 한 언어에 대응하는 LSP 서버 실행 스펙.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerSpec {
    /// 서버 풀의 키이자 UI 표기명.
    pub key: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// 이 중 하나라도 워크트리 루트에 있어야 그 언어 프로젝트로 인정한다.
    pub root_markers: &'static [&'static str],
    /// 그래프 fingerprint의 입력 — 이 파일들이 바뀌면 의미 분석 결과가 달라진다.
    pub config_files: &'static [&'static str],
    pub readiness: ReadinessSignal,
    /// `initialize` 응답 상한.
    pub init_timeout_secs: u64,
    /// 의미 분석 준비 상한.
    pub ready_timeout_secs: u64,
}

const RUST: ServerSpec = ServerSpec {
    key: "rust-analyzer",
    command: "rust-analyzer",
    args: &[],
    root_markers: &["Cargo.toml"],
    config_files: &[
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain",
        "rust-toolchain.toml",
        // `.cargo/config.toml` — cfg 플래그·target을 바꾸면 rust-analyzer의 답이 달라진다.
        // 이름으로만 매치하므로 다른 위치의 동명 파일도 걸리지만, 그쪽 오류는 불필요한
        // 재빌드로 끝나고 반대쪽 오류는 낡은 그래프를 `ready`로 남긴다.
        "config.toml",
    ],
    readiness: ReadinessSignal::ServerStatus,
    init_timeout_secs: 30,
    ready_timeout_secs: 60,
};

const TYPESCRIPT: ServerSpec = ServerSpec {
    key: "typescript-language-server",
    command: "typescript-language-server",
    args: &["--stdio"],
    root_markers: &["tsconfig.json", "jsconfig.json", "package.json"],
    config_files: &[
        "tsconfig.json",
        "jsconfig.json",
        "package.json",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
    ],
    readiness: ReadinessSignal::Progress,
    init_timeout_secs: 30,
    ready_timeout_secs: 60,
};

const PYTHON: ServerSpec = ServerSpec {
    key: "pyright",
    command: "pyright-langserver",
    args: &["--stdio"],
    root_markers: &["pyproject.toml", "setup.py", "requirements.txt"],
    config_files: &[
        "pyproject.toml",
        "setup.py",
        "setup.cfg",
        "requirements.txt",
        "Pipfile",
    ],
    readiness: ReadinessSignal::Unsupported,
    init_timeout_secs: 30,
    ready_timeout_secs: 60,
};

/// jdtls는 인자 없이 `current_dir(worktree)`로 띄운다 — 런처가 cwd로 데이터 디렉터리를
/// 유도한다. 상한이 다른 서버의 4~5배인 것은 실측에서 JVM 부팅만 1.4~2.2초였고 **큰
/// 프로젝트를 재지 못했기** 때문이다(설계 0065).
const JAVA: ServerSpec = ServerSpec {
    key: "jdtls",
    command: "jdtls",
    args: &[],
    root_markers: &["pom.xml", "build.gradle", "build.gradle.kts", ".classpath"],
    config_files: &[
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "gradle.properties",
    ],
    readiness: ReadinessSignal::LanguageStatus,
    init_timeout_secs: 120,
    ready_timeout_secs: 300,
};

/// 확장자 → (서버, LSP languageId).
///
/// languageId는 서버가 파서를 고르는 근거다. tsx를 "typescript"로 보내면 JSX 구문에서
/// 파싱이 깨져 정의를 못 찾는다 — 확장자별로 정확히 구분해 보낸다.
pub fn spec_for_path(path: &Path) -> Option<(ServerSpec, &'static str)> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let pair = match ext.as_str() {
        "rs" => (RUST, "rust"),
        "ts" | "mts" | "cts" => (TYPESCRIPT, "typescript"),
        "tsx" => (TYPESCRIPT, "typescriptreact"),
        "js" | "mjs" | "cjs" => (TYPESCRIPT, "javascript"),
        "jsx" => (TYPESCRIPT, "javascriptreact"),
        "py" | "pyi" => (PYTHON, "python"),
        "java" => (JAVA, "java"),
        _ => return None,
    };
    Some(pair)
}

/// 워크트리 루트에 이 언어의 마커 파일이 있는가.
pub fn matches_root(spec: &ServerSpec, worktree: &Path) -> bool {
    spec.root_markers
        .iter()
        .any(|marker| worktree.join(marker).is_file())
}

/// 실행 파일이 PATH에 있는가. 없으면 설치 안내를 띄워야 하므로 spawn 전에 확인한다.
pub fn command_available(command: &str) -> bool {
    if command.contains('/') {
        return Path::new(command).is_file();
    }
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(command);
        candidate.is_file() && is_executable(&candidate)
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

/// 서버를 못 쓰는 이유 — UI가 그대로 사용자에게 보여준다.
pub fn unavailable_reason(spec: &ServerSpec, worktree: &Path) -> Option<String> {
    if !matches_root(spec, worktree) {
        return Some(format!(
            "이 워크트리에서 {} 프로젝트를 찾지 못했습니다 ({} 없음)",
            spec.key,
            spec.root_markers.join(" / ")
        ));
    }
    if !command_available(spec.command) {
        return Some(format!(
            "{}이(가) PATH에 없습니다 — 설치 후 다시 시도하세요",
            spec.command
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn tsx_gets_react_language_id() {
        let (spec, lang) = spec_for_path(&PathBuf::from("src/App.tsx")).unwrap();
        assert_eq!(spec.key, "typescript-language-server");
        assert_eq!(lang, "typescriptreact");
    }

    #[test]
    fn plain_ts_is_not_react() {
        assert_eq!(
            spec_for_path(&PathBuf::from("a/b.ts")).unwrap().1,
            "typescript"
        );
    }

    #[test]
    fn rust_and_python_map_to_own_servers() {
        assert_eq!(
            spec_for_path(&PathBuf::from("src/main.rs")).unwrap().0.key,
            "rust-analyzer"
        );
        assert_eq!(
            spec_for_path(&PathBuf::from("app.py")).unwrap().0.key,
            "pyright"
        );
    }

    #[test]
    fn java_maps_to_jdtls() {
        let (spec, lang) = spec_for_path(&PathBuf::from("src/Main.java")).unwrap();
        assert_eq!(spec.key, "jdtls");
        assert_eq!(lang, "java");
        assert_eq!(spec.readiness, ReadinessSignal::LanguageStatus);
    }

    #[test]
    fn unknown_extension_has_no_server() {
        assert!(spec_for_path(&PathBuf::from("README.md")).is_none());
        assert!(spec_for_path(&PathBuf::from("Makefile")).is_none());
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        assert!(spec_for_path(&PathBuf::from("Main.RS")).is_some());
    }

    #[test]
    fn missing_marker_reports_project_not_found() {
        let dir = crate::testtmp::dir().join("praxis-lsp-spec-test-empty");
        std::fs::create_dir_all(&dir).unwrap();
        let reason = unavailable_reason(&RUST, &dir).unwrap();
        assert!(reason.contains("Cargo.toml"), "{reason}");
    }

    #[test]
    fn absolute_command_checks_the_file_itself() {
        assert!(!command_available("/nonexistent/bin/rust-analyzer"));
        assert!(command_available("/bin/sh"));
    }
}
