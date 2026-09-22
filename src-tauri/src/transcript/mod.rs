//! 에이전트 트랜스크립트 파서 — worktree 경로로부터 세션 JSONL을 찾아 메시지 텍스트를 추출.
//! Tauri 비의존. 에이전트별 포맷 차이는 `TranscriptParser` trait로 흡수.
//!
//! Claude Code: `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`,
//! encoded-cwd = canonicalize(cwd)의 `/`·`.`를 모두 `-`로 치환 (Phase 3 게이트로 검증).

use std::path::{Path, PathBuf};

/// 추출된 트랜스크립트 요약(메모리 추출 입력).
#[derive(Debug, Clone)]
pub struct TranscriptDigest {
    pub session_id: String,
    pub text: String,
    pub message_count: usize,
}

/// 에이전트별 트랜스크립트 파서.
pub trait TranscriptParser {
    fn extract(&self, worktree_path: &Path) -> anyhow::Result<Option<TranscriptDigest>>;
}

pub struct ClaudeCodeParser;

impl ClaudeCodeParser {
    /// worktree cwd → Claude Code projects 디렉터리 경로.
    ///
    /// 경로 산출·인코딩은 `sessionhome`이 소유한다(설계 결정 1). 여기서는 **canonicalize 후
    /// 실패하면 에러 전파**라는 지금 동작을 그대로 유지한다 — `sessionhome::encode_cwd`는
    /// 문자열 변환만 하므로 canonicalize 여부·실패 처리는 호출자(여기) 책임이다.
    pub fn projects_dir(worktree_path: &Path) -> anyhow::Result<PathBuf> {
        let canon = std::fs::canonicalize(worktree_path)?;
        let encoded = crate::sessionhome::encode_cwd(&canon.to_string_lossy());
        let root = crate::sessionhome::projects_root()
            .ok_or_else(|| anyhow::anyhow!("HOME/USERPROFILE 미설정"))?;
        Ok(root.join(encoded))
    }

    /// 디렉터리에서 가장 최근 .jsonl 파일 경로.
    fn latest_jsonl(dir: &Path) -> anyhow::Result<Option<PathBuf>> {
        if !dir.exists() {
            return Ok(None);
        }
        let mut latest: Option<(std::time::SystemTime, PathBuf)> = None;
        for entry in std::fs::read_dir(dir)? {
            let p = entry?.path();
            if p.extension().map(|e| e == "jsonl").unwrap_or(false) {
                let m = p.metadata()?.modified()?;
                if latest.as_ref().map(|(t, _)| m > *t).unwrap_or(true) {
                    latest = Some((m, p));
                }
            }
        }
        Ok(latest.map(|(_, p)| p))
    }
}

impl TranscriptParser for ClaudeCodeParser {
    fn extract(&self, worktree_path: &Path) -> anyhow::Result<Option<TranscriptDigest>> {
        let dir = Self::projects_dir(worktree_path)?;
        let Some(path) = Self::latest_jsonl(&dir)? else {
            return Ok(None);
        };
        parse_jsonl_str(&std::fs::read_to_string(&path)?)
    }
}

/// JSONL 문자열에서 user/assistant 메시지 텍스트를 연결. (파서 테스트용 공개)
pub fn parse_jsonl_str(content: &str) -> anyhow::Result<Option<TranscriptDigest>> {
    let mut text = String::new();
    let mut session_id = String::new();
    let mut count = 0usize;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // 깨진 라인은 스킵 (best-effort)
        };
        if session_id.is_empty() {
            if let Some(s) = v.get("sessionId").and_then(|x| x.as_str()) {
                session_id = s.to_string();
            }
        }
        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        if ty == "user" || ty == "assistant" {
            if let Some(t) = message_text(&v) {
                if !t.trim().is_empty() {
                    text.push_str(ty);
                    text.push_str(": ");
                    text.push_str(t.trim());
                    text.push('\n');
                    count += 1;
                }
            }
        }
    }
    if count == 0 {
        return Ok(None);
    }
    Ok(Some(TranscriptDigest {
        session_id,
        text,
        message_count: count,
    }))
}

/// message.content 추출: string 또는 [{type:"text", text:"..."}] 형태 모두 지원.
fn message_text(v: &serde_json::Value) -> Option<String> {
    let content = v.get("message").and_then(|m| m.get("content"))?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        let mut out = String::new();
        for block in arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    out.push_str(t);
                    out.push(' ');
                }
            }
        }
        if !out.is_empty() {
            return Some(out);
        }
    }
    None
}
