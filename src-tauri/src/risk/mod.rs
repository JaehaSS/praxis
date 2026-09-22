//! Blast Radius — 변경 파일을 위험도(high/medium/low)로 분류 (Framein blast.ts 이식).
//! Tauri 비의존, zero-dep(정규식 없이 소문자 substring). `cargo test`.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub category: String,
    pub file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Blast {
    pub level: String, // "high" | "medium" | "low"
    pub hits: Vec<Hit>,
    pub gates: Vec<String>,
}

/// (category, [substrings], level, required_gate)
const RULES: &[(&str, &[&str], &str, &str)] = &[
    (
        "secrets",
        &[".env", "secret", "credential", ".pem", ".key"],
        "high",
        "secret scan",
    ),
    (
        "auth",
        &[
            "auth",
            "login",
            "session",
            "password",
            "token",
            "oauth",
            "permission",
        ],
        "high",
        "security review",
    ),
    (
        "payment",
        &["payment", "billing", "charge", "stripe", "invoice"],
        "high",
        "payment review",
    ),
    (
        "migration",
        &["migration", "migrate", "schema", ".sql"],
        "high",
        "migration rollback plan",
    ),
    (
        "deploy",
        &[
            "dockerfile",
            "deploy",
            ".github/workflows",
            "k8s",
            "helm",
            "terraform",
        ],
        "high",
        "deploy review",
    ),
    (
        "deps",
        &[
            "package.json",
            "cargo.toml",
            "go.mod",
            "requirements.txt",
            "pnpm-lock",
            "yarn.lock",
        ],
        "medium",
        "dependency audit",
    ),
    (
        "config",
        &["config", ".toml", ".yaml", ".yml", ".ini"],
        "medium",
        "config review",
    ),
];

fn rank(level: &str) -> u8 {
    match level {
        "high" => 2,
        "medium" => 1,
        _ => 0,
    }
}

/// 변경 파일 목록 → Blast 평가. 최고 위험도 + 히트 + 필요 게이트.
pub fn assess_blast(changed: &[String]) -> Blast {
    let mut hits: Vec<Hit> = Vec::new();
    let mut gates: Vec<String> = Vec::new();
    let mut level = "low".to_string();
    for f in changed {
        let lower = f.to_lowercase();
        for (cat, pats, lvl, gate) in RULES {
            if pats.iter().any(|p| lower.contains(p)) {
                hits.push(Hit {
                    category: (*cat).to_string(),
                    file: f.clone(),
                });
                if !gates.iter().any(|g| g == gate) {
                    gates.push((*gate).to_string());
                }
                if rank(lvl) > rank(&level) {
                    level = (*lvl).to_string();
                }
            }
        }
    }
    Blast { level, hits, gates }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_for_auth_and_secrets() {
        let b = assess_blast(&["src/auth/login.rs".into()]);
        assert_eq!(b.level, "high");
        assert!(b.gates.iter().any(|g| g == "security review"));
    }

    #[test]
    fn medium_for_deps_only() {
        let b = assess_blast(&["package.json".into()]);
        assert_eq!(b.level, "medium");
        assert!(b.gates.iter().any(|g| g == "dependency audit"));
    }

    #[test]
    fn low_for_plain_files() {
        let b = assess_blast(&["README.md".into(), "src/util.rs".into()]);
        assert_eq!(b.level, "low");
        assert!(b.hits.is_empty());
    }

    #[test]
    fn max_level_wins() {
        let b = assess_blast(&["package.json".into(), "db/migration/001.sql".into()]);
        assert_eq!(b.level, "high", "migration(high) > deps(medium)");
    }
}
