//! 청킹 검증.

use crate::knowledge::chunk::{split_markdown, OVERLAP_CHARS, TARGET_CHARS};

#[test]
fn keeps_the_heading_path_as_context() {
    let chunks = split_markdown("# 설계\n본문A\n\n## 검색\n본문B");
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].heading.as_deref(), Some("설계"));
    assert_eq!(chunks[1].heading.as_deref(), Some("설계 > 검색"));
    assert!(chunks[1].content.contains("본문B"));
}

#[test]
fn sibling_heading_replaces_rather_than_nests() {
    // `## A` 다음 `## B`는 A의 하위가 아니다. 경로가 계속 쌓이면
    // "설계 > 검색 > 색인 > 배포"처럼 실제 구조와 무관한 문자열이 된다.
    let chunks = split_markdown("# 문서\n## A\n가\n## B\n나");
    assert_eq!(chunks.last().unwrap().heading.as_deref(), Some("문서 > B"));
}

#[test]
fn never_splits_inside_a_code_fence() {
    // 코드 블록이 중간에 잘리면 검색 결과가 문법적으로 깨진 조각을 보여준다.
    let long_code = format!("```rust\n{}```", "let x = 1;\n".repeat(300));
    let chunks = split_markdown(&format!("# 제목\n{long_code}"));
    let fences: usize = chunks
        .iter()
        .map(|c| c.content.matches("```").count())
        .sum();
    assert_eq!(fences % 2, 0, "펜스가 홀수 — 블록 중간에서 잘렸다");
}

#[test]
fn hash_tag_is_not_a_heading() {
    // Obsidian의 `#태그`는 heading이 아니다. heading으로 오인하면 태그 한 줄마다
    // 청크가 쪼개져 문맥이 사라진다.
    let chunks = split_markdown("# 진짜제목\n본문\n#태그아님\n계속");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.contains("#태그아님"));
}

#[test]
fn long_text_overlaps_between_consecutive_chunks() {
    // overlap이 없으면 경계에 걸친 문장이 어느 청크에서도 온전히 검색되지 않는다.
    let body = "가나다라마바사아자차\n".repeat(400);
    let chunks = split_markdown(&body);
    assert!(chunks.len() > 1, "분할되지 않았다: {}", chunks.len());

    let first: Vec<char> = chunks[0].content.chars().collect();
    let carry: String = first[first.len().saturating_sub(OVERLAP_CHARS / 2)..]
        .iter()
        .collect();
    assert!(
        chunks[1].content.contains(carry.trim()),
        "다음 청크가 이전 꼬리를 포함하지 않는다"
    );
}

#[test]
fn chunks_stay_near_the_target_size() {
    // 상한을 크게 넘으면 임베딩 모델의 입력 길이를 초과해 뒷부분이 조용히 잘린다.
    let body = "문장입니다. ".repeat(2000);
    for c in split_markdown(&body) {
        assert!(
            c.content.chars().count() < TARGET_CHARS * 2,
            "청크가 목표의 2배를 넘었다: {}",
            c.content.chars().count()
        );
    }
}

#[test]
fn ordinals_are_dense_and_zero_based() {
    // `ord`는 UNIQUE(node_id, ord) 키다. 구멍이 생기면 재색인 시 교체가 어긋난다.
    let chunks = split_markdown("# A\n가\n## B\n나\n## C\n다");
    let ords: Vec<i64> = chunks.iter().map(|c| c.ord).collect();
    assert_eq!(ords, (0..chunks.len() as i64).collect::<Vec<_>>());
}

#[test]
fn empty_input_yields_no_chunks() {
    assert!(split_markdown("").is_empty());
    assert!(split_markdown("   \n\n  ").is_empty());
}
