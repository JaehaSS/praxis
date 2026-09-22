//! Readback verification for an applied managed-memory block.

use std::path::Path;

use sha2::{Digest, Sha256};

use super::receipt::{JournalRow, MemoryReceipt};
use super::{MARK_END, MARK_START};

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn exact_managed_block(text: &str) -> Option<&str> {
    if text.matches(MARK_START).count() != 1 || text.matches(MARK_END).count() != 1 {
        return None;
    }
    let start = text.find(MARK_START)?;
    let end = text[start..].find(MARK_END)? + start + MARK_END.len();
    Some(&text[start..end])
}

/// 투영이 **자기가 쓴 바이트**에 대해서만 책임진다.
///
/// 예전에는 대상 파일 전체를 보고 "마커가 보이는데 내 영수증은 비었다"를 손상으로 단정했다.
/// 한 워크트리에 투영이 하나뿐이라는 전제 위에서만 맞는 말이다. 워크트리 없이 같은 폴더를 쓰는
/// 작업이 여럿이면 그 마커는 대개 **옆 작업이 정상으로 띄운 블록**이고, 그것이 떠 있는 동안
/// 나머지 작업 전부가 재개할 때마다(`existing_projection`) 회수할 때마다 실패했다.
///
/// 그래서 셋을 가른다. **짝이 깨진 마커**는 누구의 것도 아니므로 영수증이 비었든 아니든 손상이다.
/// **정상적인 블록 한 쌍**은, 내가 쓴 것이 없더라도 **살아 있는 이웃의 해시와 일치할 때만** 이웃의
/// 것이다. 소유자가 없는 블록은 영수증이 비었어도 실패다 — 그 파일은 이 작업의 에이전트가 읽는
/// 컨텍스트이고, 누구도 책임지지 않는 바이트가 거기 있다면 그것은 변조다.
pub(super) fn verify_projection(
    journal: &JournalRow,
    live_elsewhere: &std::collections::HashSet<String>,
) -> anyhow::Result<usize> {
    let receipts: Vec<MemoryReceipt> = serde_json::from_str(&journal.ordered_memories_json)?;
    let targets: Vec<String> = serde_json::from_str(&journal.target_paths_json)?;
    let references = targets.iter().map(String::as_str).collect::<Vec<_>>();
    let current =
        crate::projector::capture_targets(Path::new(&journal.worktree_path), &references)?;
    for target in current {
        let content = target.content.as_deref().unwrap_or_default();
        let block = exact_managed_block(content);
        if block.is_none() && (content.contains(MARK_START) || content.contains(MARK_END)) {
            anyhow::bail!("damaged managed memory marker remains");
        }
        if receipts.is_empty() {
            if let Some(block) = block {
                if !live_elsewhere.contains(&sha256(block.as_bytes())) {
                    anyhow::bail!("managed memory block has no live owner");
                }
            }
            continue;
        }
        let Some(block) = block else {
            anyhow::bail!("expected managed memory block is missing");
        };
        if sha256(block.as_bytes()) != journal.target_hash {
            anyhow::bail!("projected context no longer matches its receipt");
        }
    }
    Ok(receipts.len())
}

#[cfg(test)]
mod tests {
    use super::{sha256, verify_projection, MARK_END, MARK_START};
    use crate::memory::receipt::JournalRow;

    fn empty_receipt_journal(worktree: &std::path::Path) -> JournalRow {
        JournalRow {
            id: 1,
            task_id: 7,
            state: "applied".to_string(),
            worktree_path: worktree.to_string_lossy().into_owned(),
            target_paths_json: r#"["CLAUDE.md"]"#.to_string(),
            target_hash: sha256(b""),
            renderer_version: 3,
            ordered_memories_json: "[]".to_string(),
            preimages_json: None,
        }
    }

    /// 이웃이 띄운 블록이 있는 폴더에 메모리 0건 작업의 대상 파일을 만든다.
    fn worktree_with_block(tag: &str) -> (std::path::PathBuf, String) {
        let root = crate::testtmp::dir().join(format!(
            "praxis-projection-verify-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let block = format!("{MARK_START}\n- neighbour\n{MARK_END}");
        std::fs::write(root.join("CLAUDE.md"), format!("{block}\n# owner\n")).unwrap();
        (root, block)
    }

    /// 설계 0118이 고친 것 — 옆 작업이 살아 있는 동안 내 검증이 막히지 않는다.
    #[test]
    fn empty_receipt_accepts_a_block_a_live_neighbour_owns() {
        let (root, block) = worktree_with_block("live-neighbour");
        let live = std::collections::HashSet::from([sha256(block.as_bytes())]);

        assert_eq!(
            verify_projection(&empty_receipt_journal(&root), &live).unwrap(),
            0
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn empty_receipt_rejects_a_block_nobody_owns() {
        let (root, _) = worktree_with_block("no-owner");

        assert!(verify_projection(&empty_receipt_journal(&root), &Default::default()).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
