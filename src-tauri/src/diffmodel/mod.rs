//! Structured unified-diff parsing and hunk policy annotations.

use serde::Serialize;

mod parser;
pub use parser::parse_unified;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "text", rename_all = "lowercase")]
pub enum DiffLine {
    Context(String),
    Add(String),
    Del(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffHunk {
    pub id: String,
    pub path: String,
    pub old_range: (u32, u32),
    pub new_range: (u32, u32),
    pub lines: Vec<DiffLine>,
    pub protected: bool,
    /// 이미 커밋된 변경인가.
    ///
    /// `protected`와 나란히 있지만 뜻은 정반대다. protected는 "골라선 안 된다 → 되돌린다"이고
    /// committed는 **"손대선 안 된다"**다. 유지도 폐기도 대상이 아니다.
    /// 파서는 이 값을 모른다 — 두 범위의 diff를 비교하는 상위 계층이 채운다.
    #[serde(default)]
    pub committed: bool,
    pub risk: RiskLevel,
}

pub fn overlaps(left: &DiffHunk, right: &DiffHunk) -> bool {
    if left.path != right.path {
        return false;
    }
    let left_range = (left.new_range.0, left.new_range.0 + left.new_range.1.max(1));
    let right_range = (
        right.new_range.0,
        right.new_range.0 + right.new_range.1.max(1),
    );
    left_range.0 < right_range.1 && right_range.0 < left_range.1
}

pub fn mark_protected(hunks: &mut [DiffHunk], patterns: &[String]) {
    if patterns.is_empty() {
        return;
    }
    let mut paths = Vec::new();
    for hunk in hunks.iter() {
        if !paths.iter().any(|path| path == &hunk.path) {
            paths.push(hunk.path.clone());
        }
    }
    let violations = crate::goal_contract::protected_path_violations(patterns, &paths);
    for hunk in hunks {
        hunk.protected = violations.iter().any(|path| path == &hunk.path);
    }
}

pub fn annotate_risk(hunks: &mut [DiffHunk]) {
    let mut cache = std::collections::HashMap::new();
    for hunk in hunks {
        let risk = cache
            .entry(hunk.path.clone())
            .or_insert_with(|| risk_level_for_path(&hunk.path));
        hunk.risk = *risk;
    }
}

pub fn build_hunks(diff_text: &str, protected_patterns: &[String]) -> Vec<DiffHunk> {
    let mut hunks = parse_unified(diff_text);
    mark_protected(&mut hunks, protected_patterns);
    annotate_risk(&mut hunks);
    hunks
}

fn risk_level_for_path(path: &str) -> RiskLevel {
    let blast = crate::risk::assess_blast(std::slice::from_ref(&path.to_string()));
    match blast.level.as_str() {
        "high" => RiskLevel::High,
        "medium" => RiskLevel::Medium,
        _ => RiskLevel::Low,
    }
}

#[cfg(test)]
mod tests;
