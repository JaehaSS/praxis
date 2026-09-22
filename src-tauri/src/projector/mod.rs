//! 멀티벤더 컨텍스트 투영 (Framein projector/fileWriter 이식) — Tauri 비의존.
//!
//! managed block을 AGENTS.md에 **마커 영역만** upsert(없으면 생성).
//! 옛 투영이 남긴 CLAUDE.md·GEMINI.md는 회수·스캔 후보로만 남는다(`all_targets`).
//! 마커 밖은 사용자 소유 → 절대 손상하지 않음(핵심 안전 불변식).

use std::path::Path;

mod safe_write;
pub use safe_write::{apply_updates, capture_targets, restore_matching, TargetPreimage};

/// 기존 텍스트의 마커 블록을 교체, 없으면 추가. **마커 밖 사용자 내용은 보존.**
/// 정상(start<end): 제자리 교체. 손상(orphan/역순): stray 마커 제거 후 prepend(이중 마커 방지).
/// 주의: 마커 문자열은 예약어 — 사용자 산문에 그대로 쓰면 안 됨.
pub fn upsert_managed_block(existing: &str, start: &str, end: &str, block: &str) -> String {
    if let (Some(s), Some(e)) = (existing.find(start), existing.find(end)) {
        if e > s {
            let end_i = e + end.len();
            let mut out = String::new();
            out.push_str(&existing[..s]);
            out.push_str(block.trim_end());
            out.push_str(&existing[end_i..]);
            return out;
        }
    }
    // 마커 없음(정상) 또는 손상(한쪽만/역순) — 손상 시 stray 마커만 제거(내용 보존), 그 후 prepend.
    let has_orphan = existing.contains(start) || existing.contains(end);
    let body = if has_orphan {
        existing.replace(start, "").replace(end, "")
    } else {
        existing.to_string()
    };
    if body.trim().is_empty() {
        format!("{}\n", block.trim_end())
    } else {
        format!("{}\n\n{}", block.trim_end(), body)
    }
}

/// 정상적인 Praxis 관리 블록만 제거한다. 마커가 손상됐거나 없으면 사용자 내용을 건드리지 않는다.
pub fn remove_managed_block(existing: &str, start: &str, end: &str) -> String {
    let (Some(s), Some(e)) = (existing.find(start), existing.find(end)) else {
        return existing.to_string();
    };
    if e <= s {
        return existing.to_string();
    }
    let end_i = e + end.len();
    format!("{}{}", &existing[..s], &existing[end_i..])
}

/// 컨텍스트 파일 후보 전체(고정 순서: CLAUDE.md → AGENTS.md → GEMINI.md).
/// **쓰기 집합이 아니다** — 스캔·회수·git exclude처럼 "이미 있는 파일"을 다루는 경로 전용이다.
/// CLAUDE.md·GEMINI.md는 더 이상 만들지 않지만(아래 `WRITE_TARGETS`), 과거 투영이 남긴 파일이
/// 회수·리포트에서 빠지면 블록이 영구히 남으므로 후보로는 계속 센다.
const ALL_TARGETS: [&str; 3] = ["CLAUDE.md", "AGENTS.md", "GEMINI.md"];

/// 투영이 **새로 만들 수 있는** 파일 전체 — AGENTS.md 하나다.
/// Claude Code도 AGENTS.md를 읽게 되면서(ADR 2026-09-19) 벤더별 파일을 나눌 이유가 없어졌다.
/// 같은 블록을 두 파일에 쓰면 한쪽만 고쳐진 채 어긋나고, 승인 diff에 두 번 섞인다.
const WRITE_TARGETS: [&str; 1] = ["AGENTS.md"];

/// 투영 대상 파일. 에이전트·설정 무관하게 AGENTS.md 하나다 —
/// 옛 "벤더 인지형 타겟팅"(ADR 0009 §B)과 멀티벤더 토글은 ADR 2026-09-19로 물러났다.
pub fn project_targets() -> Vec<&'static str> {
    WRITE_TARGETS.to_vec()
}

/// 후보 컨텍스트 파일 전체 — 주입 검증 리포트처럼 "실제로 존재하는 파일"을 실측해야 하는 곳에서
/// 사용. 더 이상 만들지 않는 CLAUDE.md·GEMINI.md도 포함한다 — 옛 파일의 회수 경로를 유지하기 위함.
pub fn all_targets() -> Vec<&'static str> {
    ALL_TARGETS.to_vec()
}

/// 마커 블록을 대상 파일들에 upsert. 파일 없으면 생성, 마커 밖은 보존.
pub fn write_block(
    worktree: &Path,
    targets: &[&str],
    start: &str,
    end: &str,
    block: &str,
) -> std::io::Result<()> {
    let preimages = capture_targets(worktree, targets)?;
    let updates = preimages
        .iter()
        .map(|preimage| {
            Some(upsert_managed_block(
                preimage.content.as_deref().unwrap_or_default(),
                start,
                end,
                block,
            ))
        })
        .collect::<Vec<_>>();
    apply_updates(worktree, &preimages, &updates)
}

/// 자격 있는 지식이 없을 때 기존 Praxis 블록만 제거한다. 대상 파일이 없으면 만들지 않는다.
pub fn remove_block(
    worktree: &Path,
    targets: &[&str],
    start: &str,
    end: &str,
) -> std::io::Result<()> {
    let preimages = capture_targets(worktree, targets)?;
    let updates = preimages
        .iter()
        .map(|preimage| {
            preimage
                .content
                .as_deref()
                .map(|content| remove_managed_block(content, start, end))
        })
        .collect::<Vec<_>>();
    apply_updates(worktree, &preimages, &updates)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "<!-- praxis:begin -->";
    const E: &str = "<!-- praxis:end -->";

    #[test]
    fn insert_into_empty() {
        let out = upsert_managed_block("", S, E, &format!("{S}\nhi\n{E}"));
        assert!(out.contains("hi"));
        assert!(out.starts_with(S));
    }

    #[test]
    fn replace_block_preserves_outside() {
        let existing = format!("# 사용자 헤더\n\n{S}\nOLD\n{E}\n\n사용자 꼬리");
        let out = upsert_managed_block(&existing, S, E, &format!("{S}\nNEW\n{E}"));
        assert!(out.contains("NEW"));
        assert!(!out.contains("OLD"), "옛 블록 교체");
        assert!(out.contains("# 사용자 헤더"), "앞 사용자 내용 보존");
        assert!(out.contains("사용자 꼬리"), "뒤 사용자 내용 보존");
    }

    #[test]
    fn prepend_when_no_marker() {
        let existing = "사용자가 쓴 AGENTS.md".to_string();
        let out = upsert_managed_block(&existing, S, E, &format!("{S}\nBLK\n{E}"));
        assert!(out.contains("BLK"));
        assert!(out.contains("사용자가 쓴 AGENTS.md"), "기존 파일 보존");
    }

    #[test]
    fn remove_block_preserves_user_content() {
        let existing = format!("# user\n\n{S}\nmanaged\n{E}\n\nfooter");
        let out = remove_managed_block(&existing, S, E);
        assert!(!out.contains("managed"));
        assert!(out.contains("# user"));
        assert!(out.contains("footer"));
    }

    #[test]
    fn orphan_start_marker_stripped() {
        // 손상: start만 있고 end 없음 → stray start 제거 + 내용 보존, 마커 중복 없음.
        let existing = format!("{S}\n끊긴 블록\n사용자 내용");
        let out = upsert_managed_block(&existing, S, E, &format!("{S}\nNEW\n{E}"));
        assert!(out.contains("NEW"));
        assert!(out.contains("사용자 내용"), "내용 보존");
        assert_eq!(
            out.matches(S).count(),
            1,
            "start 마커 1개만(중복/orphan 없음)"
        );
    }

    #[test]
    fn inverted_markers_recovered() {
        // 손상: end가 start보다 앞 → 둘 다 제거 + 내용 보존 + 새 블록 1개.
        let existing = format!("{E}\n중간\n{S}");
        let out = upsert_managed_block(&existing, S, E, &format!("{S}\nNEW\n{E}"));
        assert!(out.contains("NEW"));
        assert!(out.contains("중간"), "내용 보존");
        assert_eq!(out.matches(S).count(), 1);
        assert_eq!(out.matches(E).count(), 1);
    }

    #[test]
    fn targets_is_agents_md_only() {
        assert_eq!(project_targets(), vec!["AGENTS.md"]);
    }

    /// 후보 집합은 쓰기 집합보다 넓다 — 옛 투영이 남긴 CLAUDE.md·GEMINI.md를 회수·리포트가 계속 봐야 한다.
    #[test]
    fn all_targets_keeps_legacy_files_for_cleanup() {
        assert_eq!(all_targets(), vec!["CLAUDE.md", "AGENTS.md", "GEMINI.md"]);
        assert!(!project_targets().contains(&"CLAUDE.md"));
        assert!(!project_targets().contains(&"GEMINI.md"));
    }
}
