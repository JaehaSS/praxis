use sha2::{Digest, Sha256};

use super::{DiffHunk, DiffLine, RiskLevel};

struct PendingHunk {
    old_range: (u32, u32),
    new_range: (u32, u32),
    lines: Vec<DiffLine>,
}

pub fn parse_unified(diff: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut old_path = None;
    let mut new_path = None;
    let mut current = None;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            finalize_hunk(&mut current, &mut hunks, &old_path, &new_path);
            old_path = None;
            new_path = None;
            continue;
        }
        if line.starts_with("Binary files ") {
            finalize_hunk(&mut current, &mut hunks, &old_path, &new_path);
            continue;
        }
        if let Some(raw) = line.strip_prefix("--- ") {
            old_path = parse_side_path(raw);
            continue;
        }
        if let Some(raw) = line.strip_prefix("+++ ") {
            new_path = parse_side_path(raw);
            continue;
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            finalize_hunk(&mut current, &mut hunks, &old_path, &new_path);
            current = parse_hunk_header(header);
            continue;
        }
        if !line.starts_with('\\') {
            append_hunk_line(current.as_mut(), line);
        }
    }
    finalize_hunk(&mut current, &mut hunks, &old_path, &new_path);
    hunks
}

fn append_hunk_line(current: Option<&mut PendingHunk>, line: &str) {
    let Some(pending) = current else {
        return;
    };
    if let Some(text) = line.strip_prefix('+') {
        pending.lines.push(DiffLine::Add(text.to_string()));
    } else if let Some(text) = line.strip_prefix('-') {
        pending.lines.push(DiffLine::Del(text.to_string()));
    } else if let Some(text) = line.strip_prefix(' ') {
        pending.lines.push(DiffLine::Context(text.to_string()));
    }
}

fn finalize_hunk(
    current: &mut Option<PendingHunk>,
    hunks: &mut Vec<DiffHunk>,
    old_path: &Option<String>,
    new_path: &Option<String>,
) {
    let Some(pending) = current.take() else {
        return;
    };
    let path = new_path
        .clone()
        .or_else(|| old_path.clone())
        .unwrap_or_default();
    let id = hunk_id(&path, pending.old_range, pending.new_range, &pending.lines);
    hunks.push(DiffHunk {
        id,
        path,
        old_range: pending.old_range,
        new_range: pending.new_range,
        lines: pending.lines,
        protected: false,
        // 파서는 커밋 여부를 모른다 — 두 범위를 비교하는 상위 계층이 채운다.
        committed: false,
        risk: RiskLevel::Low,
    });
}

fn parse_side_path(raw: &str) -> Option<String> {
    let raw = raw.split('\t').next().unwrap_or(raw).trim();
    if raw == "/dev/null" {
        return None;
    }
    let stripped = raw
        .strip_prefix("a/")
        .or_else(|| raw.strip_prefix("b/"))
        .unwrap_or(raw);
    Some(stripped.to_string())
}

fn parse_hunk_header(header: &str) -> Option<PendingHunk> {
    let ranges = header.split(" @@").next()?;
    let mut parts = ranges.split(' ').filter(|part| !part.is_empty());
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    Some(PendingHunk {
        old_range: parse_range(old)?,
        new_range: parse_range(new)?,
        lines: Vec::new(),
    })
}

fn parse_range(value: &str) -> Option<(u32, u32)> {
    let mut parts = value.splitn(2, ',');
    let start = parts.next()?.parse().ok()?;
    let count = match parts.next() {
        Some(count) => count.parse().ok()?,
        None => 1,
    };
    Some((start, count))
}

fn hunk_id(path: &str, old_range: (u32, u32), new_range: (u32, u32), lines: &[DiffLine]) -> String {
    let mut source = format!(
        "{path}|{}-{}|{}-{}",
        old_range.0, old_range.1, new_range.0, new_range.1
    );
    for line in lines {
        match line {
            DiffLine::Add(text) => {
                source.push_str("\n+");
                source.push_str(text.trim_end());
            }
            DiffLine::Del(text) => {
                source.push_str("\n-");
                source.push_str(text.trim_end());
            }
            DiffLine::Context(_) => {}
        }
    }
    let digest = Sha256::digest(source.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
