//! 에이전트 spawn에 인앱 MCP 서버 정의를 주입하는 인자·환경·설정 빌더(설계 0058 D-2).
//!
//! 토큰은 **argv에 절대 넣지 않는다**. claude는 설정 파일의 `${VAR}` 확장으로, codex는
//! `bearer_token_env_var`로 환경변수를 읽는다(Task 0 실측).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::convo::Vendor;
use crate::preview_bridge::mcp::ControlTokens;
use crate::preview_bridge::random_hex_id;

/// 두 벤더가 함께 읽는 Bearer 토큰 환경변수 이름.
pub const TOKEN_ENV: &str = "PRAXIS_PREVIEW_TOKEN";
/// codex 설정 키에 들어가는 서버 이름 — TOML bare key여야 해서 하이픈을 못 쓴다.
pub const SERVER_NAME: &str = "praxis_preview";
/// claude 설정 파일의 서버 키. TOML 제약이 없어 실측 그대로 하이픈을 유지한다.
const CLAUDE_SERVER_KEY: &str = "praxis-preview";
/// 툴 호출 상한. claude는 ms(env `MCP_TOOL_TIMEOUT`), codex는 초(`tool_timeout_sec`).
const TOOL_TIMEOUT_SECS: u64 = 90;
/// 기동 시 청소 기준 — 토큰 TTL과 같다. 그보다 어린 파일은 살아있는 턴의 것일 수 있다.
pub const CONFIG_TTL: Duration = Duration::from_secs(24 * 60 * 60);

pub fn endpoint_url(port: u16, instance: &str) -> String {
    format!("http://127.0.0.1:{port}/mcp/{instance}")
}

/// 모든 인스턴스의 설정이 모이는 뿌리. 기동 청소가 이 디렉터리를 훑는다.
pub fn config_root(data_dir: &Path) -> PathBuf {
    data_dir.join("mcp-config")
}

pub fn config_dir(data_dir: &Path, instance: &str) -> PathBuf {
    config_root(data_dir).join(instance)
}

/// claude `--mcp-config` 본문. 토큰 자리에는 **플레이스홀더만** 들어간다.
pub fn claude_config_json(url: &str) -> String {
    serde_json::json!({
        "mcpServers": {
            CLAUDE_SERVER_KEY: {
                "type": "http",
                "url": url,
                "headers": { "Authorization": format!("Bearer ${{{TOKEN_ENV}}}") },
            }
        }
    })
    .to_string()
}

pub fn vendor_args(vendor: Vendor, url: &str, config_path: &Path) -> Vec<String> {
    match vendor {
        // `--strict-mcp-config`는 넣지 않는다 — 프로젝트 `.mcp.json` 서버를 끊는 행동 변경.
        Vendor::Claude => vec![
            "--mcp-config".to_string(),
            config_path.to_string_lossy().into_owned(),
        ],
        Vendor::Codex => vec![
            "-c".to_string(),
            format!("mcp_servers.{SERVER_NAME}.url=\"{url}\""),
            "-c".to_string(),
            format!("mcp_servers.{SERVER_NAME}.bearer_token_env_var=\"{TOKEN_ENV}\""),
            "-c".to_string(),
            format!("mcp_servers.{SERVER_NAME}.tool_timeout_sec={TOOL_TIMEOUT_SECS}"),
        ],
        Vendor::Agy => Vec::new(),
    }
}

pub fn env_for(token: &str) -> Vec<(String, String)> {
    env_for_kind(token, false)
}

/// 질문 턴은 상한이 질문 TTL보다 커야 한다 — 90초로는 사용자가 읽기도 전에 툴이 끊긴다.
/// 같은 턴의 다른 MCP 툴에도 같은 상한이 걸린다. 실험 기능의 대가다.
pub fn env_for_kind(token: &str, questions: bool) -> Vec<(String, String)> {
    let secs = if questions {
        (crate::convo::interaction::QUESTION_TTL as u64) + 60
    } else {
        TOOL_TIMEOUT_SECS
    };
    vec![
        (TOKEN_ENV.to_string(), token.to_string()),
        ("MCP_TOOL_TIMEOUT".to_string(), (secs * 1000).to_string()),
    ]
}

/// spawn 한 번에 얹을 인자와 환경변수.
#[derive(Debug, Clone, Default)]
pub struct McpInjection {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// 벤더 인자를 마지막 위치인자(지시문) **앞**에 끼운다 — codex `exec`는 프롬프트가 마지막이라
/// 뒤에 붙이면 옵션이 프롬프트 뒤로 밀린다.
pub fn splice_before_instruction(args: &mut Vec<String>, extra: Vec<String>, instruction: &str) {
    let at = match args.last() {
        Some(last) if last == instruction => args.len() - 1,
        _ => args.len(),
    };
    args.splice(at..at, extra);
}

/// spawn 하나가 쥐는 토큰·설정 파일의 수명. 드롭되면 토큰이 죽고 파일이 사라진다.
pub struct PreviewMcpLease {
    token: String,
    spawn_id: String,
    config_path: Option<PathBuf>,
    tokens: ControlTokens,
    injection: McpInjection,
}

impl PreviewMcpLease {
    pub fn issue(
        tokens: &ControlTokens,
        task_id: i64,
        vendor: Vendor,
        endpoint: &str,
        config_dir: &Path,
    ) -> Result<Self, String> {
        Self::issue_for(tokens, task_id, vendor, endpoint, config_dir, false)
    }

    pub fn issue_for(
        tokens: &ControlTokens,
        task_id: i64,
        vendor: Vendor,
        endpoint: &str,
        config_dir: &Path,
        questions: bool,
    ) -> Result<Self, String> {
        let spawn_id = random_hex_id()?;
        let token = tokens.issue(task_id, &spawn_id)?;
        let config_path = match vendor {
            Vendor::Claude => match write_config(config_dir, &spawn_id, endpoint) {
                Ok(path) => Some(path),
                Err(error) => {
                    tokens.revoke(&token); // 설정을 못 쓰면 토큰도 남기지 않는다.
                    return Err(error);
                }
            },
            _ => None,
        };
        let args = match &config_path {
            Some(path) => vendor_args(vendor, endpoint, path),
            None => vendor_args(vendor, endpoint, Path::new("")),
        };
        let injection = McpInjection {
            args,
            env: env_for_kind(&token, questions),
        };
        Ok(Self {
            token,
            spawn_id,
            config_path,
            tokens: tokens.clone(),
            injection,
        })
    }

    pub fn injection(&self) -> &McpInjection {
        &self.injection
    }

    /// Revocation blocks new admissions; existing dispatches must also finish.
    pub fn revoke_and_drain(&self, timeout: std::time::Duration) -> bool {
        self.tokens.revoke(&self.token);
        let started = std::time::Instant::now();
        while self.tokens.active_for_spawn(&self.spawn_id) != 0 {
            if started.elapsed() >= timeout { return false; }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        true
    }

    pub fn spawn_id(&self) -> &str {
        &self.spawn_id
    }
}

impl Drop for PreviewMcpLease {
    fn drop(&mut self) {
        self.tokens.revoke(&self.token);
        if let Some(path) = &self.config_path {
            let _ = fs::remove_file(path);
        }
    }
}

fn write_config(dir: &Path, spawn_id: &str, url: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let path = dir.join(format!("{spawn_id}.json"));
    fs::write(&path, claude_config_json(url)).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    Ok(path)
}

/// 기동 청소: `root`와 그 바로 아래 인스턴스 디렉터리에서 `ttl`보다 오래된 파일만 지운다.
/// 디렉터리는 남긴다 — dev 빌드와 `/Applications`가 같은 데이터 디렉터리를 함께 쓴다.
pub fn sweep_stale(root: &Path, ttl: Duration) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            sweep_files(&entry.path(), ttl);
            continue;
        }
        remove_if_stale(&entry.path(), &meta, ttl);
    }
}

fn sweep_files(dir: &Path, ttl: Duration) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_file() {
            remove_if_stale(&entry.path(), &meta, ttl);
        }
    }
}

fn remove_if_stale(path: &Path, meta: &fs::Metadata, ttl: Duration) {
    let stale = meta
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= ttl);
    if stale {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("praxis-inject-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn endpoint_is_loopback_with_the_instance_segment() {
        assert_eq!(
            endpoint_url(62325, "inst1"),
            "http://127.0.0.1:62325/mcp/inst1"
        );
    }

    #[test]
    fn no_vendor_puts_the_token_in_argv() {
        let dir = temp_dir("argv");
        let tokens = ControlTokens::default();
        for vendor in [Vendor::Claude, Vendor::Codex, Vendor::Agy] {
            let lease = PreviewMcpLease::issue(&tokens, 1, vendor, "http://x", &dir).unwrap();
            let token = lease.injection().env[0].1.clone();
            assert!(!token.is_empty());
            assert!(!lease.injection().args.iter().any(|a| a.contains(&token)));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn claude_args_point_at_the_config_file_and_never_go_strict() {
        let args = vendor_args(Vendor::Claude, "http://x", Path::new("/tmp/a.json"));
        assert_eq!(args, vec!["--mcp-config", "/tmp/a.json"]);
        assert!(!args.iter().any(|a| a == "--strict-mcp-config"));
    }

    #[test]
    fn codex_args_are_toml_config_overrides() {
        let args = vendor_args(Vendor::Codex, "http://x", Path::new(""));
        assert_eq!(args[0], "-c");
        assert_eq!(args[1], "mcp_servers.praxis_preview.url=\"http://x\"");
        assert_eq!(
            args[3],
            "mcp_servers.praxis_preview.bearer_token_env_var=\"PRAXIS_PREVIEW_TOKEN\""
        );
        assert_eq!(args[5], "mcp_servers.praxis_preview.tool_timeout_sec=90");
        assert!(!args.iter().any(|a| a == "--strict-mcp-config"));
    }

    #[test]
    fn claude_config_carries_the_placeholder_not_a_token() {
        let json = claude_config_json("http://127.0.0.1:1/mcp/i");
        assert!(json.contains("Bearer ${PRAXIS_PREVIEW_TOKEN}"));
        assert!(json.contains("\"praxis-preview\""));
        assert!(json.contains("\"type\":\"http\""));
    }

    #[test]
    fn lease_drop_revokes_the_token_and_removes_the_config() {
        let dir = temp_dir("lease");
        let tokens = ControlTokens::default();
        let (token, path) = {
            let lease =
                PreviewMcpLease::issue(&tokens, 7, Vendor::Claude, "http://x", &dir).unwrap();
            let path = PathBuf::from(&lease.injection().args[1]);
            let body = fs::read_to_string(&path).unwrap();
            let token = lease.injection().env[0].1.clone();
            assert!(!body.contains(&token)); // 파일에 평문 토큰이 없다.
            assert_eq!(tokens.task_for(&token), Some(7));
            (token, path)
        };

        assert_eq!(tokens.task_for(&token), None);
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn codex_lease_writes_no_file() {
        let dir = temp_dir("codex");
        let tokens = ControlTokens::default();
        let lease = PreviewMcpLease::issue(&tokens, 7, Vendor::Codex, "http://x", &dir).unwrap();
        assert!(fs::read_dir(&dir).unwrap().next().is_none());
        assert!(!lease.spawn_id().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweep_removes_stale_files_in_instance_dirs_but_keeps_fresh_ones() {
        let root = temp_dir("sweep");
        let instance = root.join("inst1");
        fs::create_dir_all(&instance).unwrap();
        let file = instance.join("a.json");
        fs::write(&file, "{}").unwrap();

        sweep_stale(&root, CONFIG_TTL);
        assert!(file.exists()); // 갓 만든 파일은 살아남는다.

        sweep_stale(&root, Duration::ZERO);
        assert!(!file.exists());
        assert!(instance.exists()); // 디렉터리는 남긴다.
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn injection_args_go_before_a_trailing_instruction() {
        let mut args = vec!["exec".to_string(), "prompt".to_string()];
        splice_before_instruction(&mut args, vec!["-c".to_string()], "prompt");
        assert_eq!(args, vec!["exec", "-c", "prompt"]);

        let mut args = vec!["-p".to_string(), "prompt".to_string(), "--flag".to_string()];
        splice_before_instruction(&mut args, vec!["-c".to_string()], "prompt");
        assert_eq!(args, vec!["-p", "prompt", "--flag", "-c"]);
    }
}
