//! 워크트리 언어 감지 → LSP-MCP 브리지 스펙 (Tauri 비의존, 순수 함수).
//!
//! 루트 마커 파일로 언어를 판정하고, Phase 0에서 검증된 `jonrad/lsp-mcp` 브리지
//! 명령을 반환한다. 파일시스템 조회 외 부작용 없음 — PATH/홈 등은 호출부에서 확인 후
//! `rust_analyzer_available` 플래그로 전달한다.

use std::path::Path;

/// 자동주입 후보 LSP-MCP 서버 스펙.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspServerSpec {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

/// 공급망 고정 — `jonrad/lsp-mcp` 브리지를 특정 커밋에 핀(임의 upstream 변경으로부터 격리).
const LSP_MCP_PIN: &str =
    "git+https://github.com/jonrad/lsp-mcp#b48c04c52731e3e499352fc644992dcce6202db2";

fn npx_args(lsp: &str) -> Vec<String> {
    ["-y", "--silent", LSP_MCP_PIN, "--lsp", lsp]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn rust_spec() -> LspServerSpec {
    LspServerSpec {
        name: "lsp-rust".to_string(),
        command: "npx".to_string(),
        args: npx_args("rust-analyzer"),
    }
}

fn typescript_spec() -> LspServerSpec {
    LspServerSpec {
        name: "lsp-typescript".to_string(),
        command: "npx".to_string(),
        args: npx_args("npx -y --silent typescript-language-server --stdio"),
    }
}

fn python_spec() -> LspServerSpec {
    LspServerSpec {
        name: "lsp-python".to_string(),
        command: "npx".to_string(),
        args: npx_args("pyright-langserver --stdio"),
    }
}

/// worktree 루트 마커로 언어를 판정해 해당 LSP-MCP 브리지 스펙들을 반환한다.
/// `rust_analyzer_available`이 false면 rust 스펙은 제외한다(rust-analyzer는 PATH 바이너리
/// 필요 — npx로 받을 수 없음). typescript/python은 브리지가 npx로 서버까지 받으므로 항상 가능.
pub fn detect_lsp_servers(worktree: &Path, rust_analyzer_available: bool) -> Vec<LspServerSpec> {
    let mut out = Vec::new();
    if rust_analyzer_available && worktree.join("Cargo.toml").is_file() {
        out.push(rust_spec());
    }
    if worktree.join("tsconfig.json").is_file() || worktree.join("package.json").is_file() {
        out.push(typescript_spec());
    }
    if worktree.join("pyproject.toml").is_file() || worktree.join("setup.py").is_file() {
        out.push(python_spec());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn tmp_dir(prefix: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = crate::testtmp::dir().join(format!(
            "praxis-lspdetect-{}-{}-{}",
            std::process::id(),
            n,
            prefix
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), "").unwrap();
    }

    #[test]
    fn rust_marker_with_analyzer_available_yields_rust_spec() {
        let dir = tmp_dir("rust-ok");
        touch(&dir, "Cargo.toml");
        let servers = detect_lsp_servers(&dir, true);
        assert_eq!(servers, vec![rust_spec()]);
    }

    #[test]
    fn rust_marker_without_analyzer_available_is_excluded() {
        let dir = tmp_dir("rust-no");
        touch(&dir, "Cargo.toml");
        let servers = detect_lsp_servers(&dir, false);
        assert!(servers.is_empty());
    }

    #[test]
    fn tsconfig_marker_yields_typescript_spec() {
        let dir = tmp_dir("ts-tsconfig");
        touch(&dir, "tsconfig.json");
        let servers = detect_lsp_servers(&dir, true);
        assert_eq!(servers, vec![typescript_spec()]);
    }

    #[test]
    fn package_json_marker_yields_typescript_spec() {
        let dir = tmp_dir("ts-package");
        touch(&dir, "package.json");
        let servers = detect_lsp_servers(&dir, true);
        assert_eq!(servers, vec![typescript_spec()]);
    }

    #[test]
    fn pyproject_marker_yields_python_spec() {
        let dir = tmp_dir("py-pyproject");
        touch(&dir, "pyproject.toml");
        let servers = detect_lsp_servers(&dir, true);
        assert_eq!(servers, vec![python_spec()]);
    }

    #[test]
    fn setup_py_marker_yields_python_spec() {
        let dir = tmp_dir("py-setup");
        touch(&dir, "setup.py");
        let servers = detect_lsp_servers(&dir, true);
        assert_eq!(servers, vec![python_spec()]);
    }

    #[test]
    fn multiple_markers_yield_multiple_specs() {
        let dir = tmp_dir("multi");
        touch(&dir, "Cargo.toml");
        touch(&dir, "package.json");
        touch(&dir, "pyproject.toml");
        let servers = detect_lsp_servers(&dir, true);
        assert_eq!(servers, vec![rust_spec(), typescript_spec(), python_spec()]);
    }

    #[test]
    fn no_markers_yields_empty() {
        let dir = tmp_dir("empty");
        let servers = detect_lsp_servers(&dir, true);
        assert!(servers.is_empty());
    }
}
