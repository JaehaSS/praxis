//! knowledge_type 유형 사전·정규화 계약 — 핸드오프 축(abandoned·pitfall) 포함.

use super::{knowledge_type, normalize_knowledge_type};

#[test]
fn is_valid_accepts_new_axes() {
    assert!(knowledge_type::is_valid(knowledge_type::ABANDONED));
    assert!(knowledge_type::is_valid(knowledge_type::PITFALL));
}

#[test]
fn is_valid_rejects_unknown_and_is_case_sensitive() {
    // create_candidate/update_knowledge의 거부 경로가 이 계약에 의존한다.
    assert!(!knowledge_type::is_valid("bogus"));
    assert!(!knowledge_type::is_valid(""));
    // is_valid는 정확 일치(저장값 검증), 대소문자 흡수는 normalize의 몫 — 짝이지만 계약이 다르다.
    assert!(!knowledge_type::is_valid("ABANDONED"));
}

#[test]
fn normalize_maps_new_axes_case_insensitively() {
    assert_eq!(
        normalize_knowledge_type("abandoned"),
        knowledge_type::ABANDONED
    );
    assert_eq!(normalize_knowledge_type("PITFALL"), knowledge_type::PITFALL);
    // LLM 출력의 대소문자 변주는 capture 로컬 사본이 아니라 여기서 흡수한다.
    assert_eq!(
        normalize_knowledge_type("Decision"),
        knowledge_type::DECISION
    );
    assert_eq!(normalize_knowledge_type("뭔지모름"), knowledge_type::CLAIM);
    assert_eq!(normalize_knowledge_type(""), knowledge_type::CLAIM);
    assert_eq!(
        normalize_knowledge_type("Abandoned"),
        knowledge_type::ABANDONED
    );
    // LLM JSON의 부수 공백이 새 축을 claim으로 조용히 접지 않게 trim한다.
    assert_eq!(
        normalize_knowledge_type(" pitfall "),
        knowledge_type::PITFALL
    );
}
