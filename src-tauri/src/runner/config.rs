use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

pub const DEFAULT_BIND: &str = "127.0.0.1:47831";
pub const DEFAULT_CONCURRENCY: usize = 2;
pub const MAX_CONCURRENCY: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionPolicy {
    AlwaysApprove,
    RequireApproval,
}

impl ExecutionPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AlwaysApprove => "always_approve",
            Self::RequireApproval => "require_approval",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerConfig {
    pub bind: SocketAddr,
    pub repository_roots: Vec<PathBuf>,
    pub max_concurrent_tasks: usize,
    pub execution_policy: ExecutionPolicy,
    pub pairing_token_file: PathBuf,
}

impl RunnerConfig {
    pub fn from_toml(input: &str) -> Result<Self, String> {
        let mut bind: SocketAddr = DEFAULT_BIND
            .parse()
            .map_err(|error| format!("기본 bind 주소 오류: {error}"))?;
        let mut roots = Vec::new();
        let mut concurrency = DEFAULT_CONCURRENCY;
        let mut execution_policy = ExecutionPolicy::AlwaysApprove;
        let mut pairing_token_file = None;
        for line in input
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let Some((key, raw)) = line.split_once('=') else {
                return Err("설정 형식이 올바르지 않습니다".to_string());
            };
            let value = raw.trim();
            match key.trim() {
                "bind" => {
                    bind = value
                        .trim_matches('"')
                        .parse()
                        .map_err(|_| "bind 주소가 올바르지 않습니다".to_string())?
                }
                "repository_roots" => {
                    roots = value
                        .trim_matches(['[', ']'])
                        .split(',')
                        .filter_map(|v| {
                            let p = v.trim().trim_matches('"');
                            (!p.is_empty()).then_some(PathBuf::from(p))
                        })
                        .collect()
                }
                "max_concurrent_tasks" => {
                    concurrency = value
                        .parse()
                        .map_err(|_| "동시 실행 값이 올바르지 않습니다".to_string())?
                }
                "execution_policy" => {
                    execution_policy = match value.trim_matches('"') {
                        "always_approve" => ExecutionPolicy::AlwaysApprove,
                        "require_approval" => ExecutionPolicy::RequireApproval,
                        _ => return Err(
                            "execution_policy는 always_approve 또는 require_approval이어야 합니다"
                                .to_string(),
                        ),
                    }
                }
                "pairing_token_file" => {
                    pairing_token_file = Some(PathBuf::from(value.trim_matches('"')))
                }
                _ => return Err(format!("지원하지 않는 Runner 설정: {}", key.trim())),
            }
        }
        if !matches!(bind.ip(), IpAddr::V4(v) if v.is_loopback()) {
            return Err("Runner는 127.0.0.1에만 bind할 수 있습니다".to_string());
        }
        if !(1..=MAX_CONCURRENCY).contains(&concurrency) {
            return Err(format!("동시 실행은 1..={MAX_CONCURRENCY}만 허용합니다"));
        }
        let repository_roots = canonicalize_repository_roots(roots)?;
        let pairing_token_file =
            pairing_token_file.ok_or_else(|| "pairing_token_file이 필요합니다".to_string())?;
        Ok(Self {
            bind,
            repository_roots,
            max_concurrent_tasks: concurrency,
            execution_policy,
            pairing_token_file,
        })
    }
}

fn canonicalize_repository_roots(roots: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    if roots.is_empty() {
        return Err("repository_roots가 비어 있습니다".to_string());
    }
    roots
        .into_iter()
        .map(|root| {
            let canonical = root
                .canonicalize()
                .map_err(|_| format!("repository root를 찾을 수 없습니다: {}", root.display()))?;
            if !canonical.is_dir() {
                return Err(format!(
                    "repository root는 디렉터리여야 합니다: {}",
                    canonical.display()
                ));
            }
            Ok(canonical)
        })
        .collect()
}
