//! Deterministic rendering and strict marker topology validation.

use std::collections::HashSet;

use super::receipt::MemoryReceipt;
use super::{MARK_END, MARK_START};

fn render_entry(block: &mut String, receipt: &MemoryReceipt) -> anyhow::Result<()> {
    super::validate_knowledge_content(&receipt.content)?;
    // M-id는 인용 관측의 앵커다(설계 0048) — 세션이 이 토큰을 표기하면 턴 종료 판정이 잡는다.
    block.push_str(&format!(
        "- [M-{} · {} · verified · v{} · evidence {}] {}\n",
        receipt.memory_id,
        receipt.knowledge_type,
        receipt.version,
        receipt.evidence.len(),
        receipt.content
    ));
    Ok(())
}

/// 단일 managed block 안에 두 섹션으로 렌더한다 — 블록을 쪼개면 projector의 marker
/// topology 검사와 안전 투영이 통째로 달라진다.
///
/// 지정 규칙이 없으면 섹션 머리글 없이 기존 형태를 그대로 낸다. (과거의 바이트 불변
/// 약속은 renderer v3의 M-id 마커·인용 지시문 도입으로 끝났다 — 출력이 바뀌는 변경은
/// `RENDERER_VERSION` 승격으로 구분한다.)
pub(super) fn render_block(receipts: &[MemoryReceipt]) -> anyhow::Result<Option<String>> {
    if receipts.is_empty() {
        return Ok(None);
    }
    let must_apply = super::application_policy::policy::MUST_APPLY;
    let (designated, relevant): (Vec<_>, Vec<_>) = receipts
        .iter()
        .partition(|receipt| receipt.application_policy == must_apply);

    let mut block = format!("{MARK_START}\n# Project Memory (Praxis 자동 삽입)\n\n");
    if designated.is_empty() {
        for receipt in receipts {
            render_entry(&mut block, receipt)?;
        }
    } else {
        block.push_str("## 항상 적용\n");
        for receipt in &designated {
            render_entry(&mut block, receipt)?;
        }
        if !relevant.is_empty() {
            block.push_str("\n## 관련 메모리\n");
            for receipt in &relevant {
                render_entry(&mut block, receipt)?;
            }
        }
    }
    block.push_str("\n위 항목을 실제로 활용했다면 응답에 해당 ID(예: M-123)를 표기할 것.\n");
    block.push_str(MARK_END);
    Ok(Some(block))
}

fn validate_marker_topology(content: &str) -> anyhow::Result<()> {
    let starts = content.match_indices(MARK_START).collect::<Vec<_>>();
    let ends = content.match_indices(MARK_END).collect::<Vec<_>>();
    if starts.is_empty() && ends.is_empty() {
        return Ok(());
    }
    if starts.len() != 1 || ends.len() != 1 || starts[0].0 >= ends[0].0 {
        anyhow::bail!("projection target has malformed or duplicate memory markers");
    }
    Ok(())
}

/// 낼 블록이 있으면 마커 영역만 upsert하고, 없으면 대상에 남은 블록의 **주인을 보고** 정한다.
///
/// `live_elsewhere`는 같은 폴더에서 지금 살아 있는 다른 투영들의 블록 해시다. 블록을 낼 것이
/// 없을 때 두 가지가 똑같이 생긴 채로 파일에 남아 있기 때문에 필요하다.
///
/// - **낡은 잔재** — 지난 세션이 남긴 블록. 지워야 한다. 승인되지 않은 옛 문장이 검증된 지식인
///   척 컨텍스트에 계속 실린다.
/// - **이웃의 블록** — 같은 폴더를 워크트리 없이 공유하는 다른 작업이 방금 띄운 것. 지우면 그
///   작업은 자기 블록이 사라진 것을 보고 검증에 실패하고, 이쪽은 남의 블록을 자기 잔재로 읽어
///   또 실패한다 — 한 번의 제거가 양쪽을 깨뜨린다.
///
/// 파일만 봐서는 둘이 구별되지 않는다. **소유자가 아직 살아 있는지는 저널만 안다.**
///
/// 보존은 `preimage.content`를 그대로 내는 것으로 표현한다 — `None`은 "손대지 않음"이 아니라
/// **파일 삭제**를 뜻하므로(`safe_write::io::write_update`), 존재하는 파일에 그것을 내면
/// 사용자 파일이 사라진다.
pub(super) fn planned_updates(
    preimages: &[crate::projector::TargetPreimage],
    block: Option<&str>,
    live_elsewhere: &HashSet<String>,
) -> anyhow::Result<Vec<Option<String>>> {
    preimages
        .iter()
        .map(|preimage| {
            let content = preimage.content.as_deref().unwrap_or_default();
            // 낼 블록이 없어도 검사는 한다 — 이 파일은 어느 쪽이든 **이 작업의 에이전트가 읽는
            // 컨텍스트**다. 짝이 깨진 마커를 보고도 통과시키면 손상된 컨텍스트로 작업이 시작된다.
            validate_marker_topology(content)?;
            let Some(block) = block else {
                return Ok(cleared_unless_owned(preimage, content, live_elsewhere));
            };
            Ok(Some(crate::projector::upsert_managed_block(
                content, MARK_START, MARK_END, block,
            )))
        })
        .collect()
}

/// 낼 블록이 없을 때의 대상 처리 — 이웃이 소유한 블록이면 그대로, 아니면 잔재로 보고 걷어낸다.
fn cleared_unless_owned(
    preimage: &crate::projector::TargetPreimage,
    content: &str,
    live_elsewhere: &HashSet<String>,
) -> Option<String> {
    let owned_by_neighbour =
        super::projection_verify::exact_managed_block(content).is_some_and(|block| {
            live_elsewhere.contains(&super::projection_verify::sha256(block.as_bytes()))
        });
    if owned_by_neighbour {
        return preimage.content.clone();
    }
    preimage
        .content
        .as_ref()
        .map(|_| crate::projector::remove_managed_block(content, MARK_START, MARK_END))
}

#[cfg(test)]
mod tests;
