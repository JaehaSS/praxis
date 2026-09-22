//! 대기 인사이트 덱 — 응답을 기다리는 동안 도메인 지식 카드를 띄운다.
//!
//! **`insights` 모듈과 다른 것이다.** 그쪽은 `~/.claude/projects/*.jsonl`을 스캔하는
//! 사용량 통계다. 이름만 비슷하고 하는 일이 전혀 없이 다르므로 섞지 않는다.
//!
//! 게이트는 **새로 만들지 않는다** — `quiz::gate::should_offer`가 이미 "충분히 기다렸는가"를
//! 판정하고 프론트의 `use-quiz-gate`가 "띄울 것이 있는가"까지 본다. 인사이트는 그 판정에
//! 소스 하나를 더하는 것이다.
//!
//! 카드의 원천은 둘이다 — 덱 파일(`deck`)과 개인 지식창고 문서(`wiki`). 둘은 같은
//! `Deck` 모양으로 합쳐져 `serve::pick_next`가 구분 없이 고른다.

pub mod deck;
pub mod serve;
pub mod wiki;

/// 덱 디렉터리 이름 — 앱 데이터 디렉터리 밑(퀴즈 inbox와 같은 자리).
pub const DECKS_SUBDIR: &str = "insight-decks";
