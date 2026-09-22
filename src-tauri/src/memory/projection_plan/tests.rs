use crate::projector::TargetPreimage;

/// 이웃이 없는 경우 — marker topology 검사에는 영향이 없다.
fn none() -> std::collections::HashSet<String> {
    std::collections::HashSet::new()
}

fn preimage(content: &str) -> TargetPreimage {
    TargetPreimage {
        relative_path: "CLAUDE.md".to_string(),
        content: Some(content.to_string()),
        readonly: false,
        unix_mode: None,
    }
}

fn mem_receipt(id: i64, policy: &str, content: &str) -> crate::memory::receipt::MemoryReceipt {
    crate::memory::receipt::MemoryReceipt {
        memory_id: id,
        version: 3,
        content: content.to_string(),
        knowledge_type: "decision".to_string(),
        application_policy: policy.to_string(),
        evidence: Vec::new(),
    }
}

/// 인용 관측(설계 0048)의 전제 — 항목마다 M-id 마커, 블록 말미에 인용 지시문.
#[test]
fn render_includes_citation_marker_and_directive() {
    let block = super::render_block(&[
        mem_receipt(12, "must_apply", "머지는 항상 로컬에서 한다"),
        mem_receipt(34, "relevance", "투영은 자기 바이트만 책임진다"),
    ])
    .unwrap()
    .unwrap();
    assert!(block.contains("- [M-12 · decision · verified · v3 · evidence 0] 머지는 항상 로컬에서 한다"));
    assert!(block.contains("- [M-34 · decision · verified · v3 · evidence 0] 투영은 자기 바이트만 책임진다"));
    assert!(
        block.contains("M-12") && block.contains("활용했다면"),
        "인용 지시문이 블록 안에 있어야 한다"
    );
    assert!(block.trim_end().ends_with(super::MARK_END), "지시문은 마커 안쪽이다");
}

#[test]
fn render_without_receipts_stays_none() {
    assert!(super::render_block(&[]).unwrap().is_none());
}

#[test]
fn rejects_duplicate_or_orphan_markers() {
    let duplicate = format!(
        "{}\na\n{}\n{}\nb\n{}",
        super::MARK_START,
        super::MARK_END,
        super::MARK_START,
        super::MARK_END
    );
    let orphan = format!("owner\n{}", super::MARK_START);

    assert!(super::planned_updates(&[preimage(&duplicate)], Some("block"), &none()).is_err());
    assert!(super::planned_updates(&[preimage(&orphan)], Some("block"), &none()).is_err());
}

#[test]
fn accepts_no_markers_or_one_ordered_pair() {
    let ordered = format!("{}\nold\n{}", super::MARK_START, super::MARK_END);

    assert!(super::planned_updates(&[preimage("owner")], Some("block"), &none()).is_ok());
    assert!(super::planned_updates(&[preimage(&ordered)], Some("block"), &none()).is_ok());
}
