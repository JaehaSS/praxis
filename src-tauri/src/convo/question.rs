//! 턴 종료 텍스트가 "사용자에게 던진 질문"인지 판별 — 순수 함수(cargo test).
//!
//! 검토 대기(`AwaitingReview`) 안에서 "다 했으니 승인해달라"와 "결정을 물어본다"를 가르는
//! 유일한 근거. 상태 자체는 나누지 않고 표시용 주석(`tasks.awaiting_kind`)만 붙이므로,
//! 오판의 대가는 사이드바 점 색깔 하나다 — 승인·폐기·재개 경로는 어느 쪽이든 동일하게 열린다.
//! 이 비대칭이 휴리스틱을 허용 가능하게 만든다.

/// 질문 자체가 아니라 그 주위를 감싸는 마크다운 — 판별 대상에서 제외한다.
/// 에이전트는 "A로 할까요?" 뒤에 선택지 목록·코드블록을 붙이는 일이 잦아,
/// 마지막 줄만 그대로 보면 정작 질문을 놓친다.
fn is_ornament(line: &str) -> bool {
    if line.starts_with('>') || line.starts_with('|') {
        return true;
    }
    // 구분선 --- / *** / ___
    if line.len() >= 3 && line.chars().all(|c| matches!(c, '-' | '*' | '_')) {
        return true;
    }
    // 불릿: "- 항목", "* 항목", "+ 항목" (구분선과 달리 뒤에 내용이 온다)
    if let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
    {
        return !rest.trim().is_empty();
    }
    // 번호 목록: "1. 항목", "2) 항목"
    let digits: String = line.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        let rest = &line[digits.len()..];
        if rest.starts_with(". ") || rest.starts_with(") ") {
            return true;
        }
    }
    false
}

/// 물음표 뒤에 붙는 마크다운 강조·괄호·따옴표를 벗긴다 — `**정말 진행할까요?**`가 대표적이다.
fn strip_trailing_ornament(line: &str) -> &str {
    line.trim_end_matches(|c: char| {
        matches!(c, '*' | '_' | '`' | '"' | '\'' | ')' | ']' | '”' | '»') || c.is_whitespace()
    })
}

/// 뒤에서부터 장식을 건너뛰고 만난 첫 실질 줄.
///
/// 코드펜스는 줄이 아니라 **블록 단위**로 건너뛴다 — 펜스 줄만 걸러내면 그 안의 코드가
/// 실질 줄로 잡혀, 코드블록으로 끝나는 답변의 질문을 전부 놓친다.
fn last_substantive_line(text: &str) -> Option<&str> {
    let mut in_fence = false;
    for line in text.lines().rev().map(str::trim) {
        if line.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || line.is_empty() || is_ornament(line) {
            continue;
        }
        return Some(line);
    }
    None
}

/// 물음표 없이 답을 요구하는 정중 요청 — "어느 쪽인지 **알려주시면** 진행하겠습니다"가 대표적이다.
///
/// 실제 답변에서 이 형태가 물음표만큼 흔하다는 것이 확인돼 추가했다(원장 #198). 요청이 문장 끝이
/// 아니라 중간에 오고 뒤에 "…하겠습니다"가 붙으므로, 종결이 아니라 **줄 안 포함**으로 본다.
///
/// 동사를 "정보를 달라"는 것으로 한정한다 — `검토해 주세요`·`확인 부탁드립니다`는 결과 승인
/// 요청이라 검토 대기가 맞다. 이 구분이 없으면 완료 보고가 전부 질문으로 넘어온다.
const ASKING_PHRASES: &[&str] = &[
    "알려주시면",
    "알려주세요",
    "알려주시겠",
    "답해주시면",
    "답해주세요",
    "답변주시면",
    "답변주세요",
    "답변부탁",
    "말씀해주시면",
    "말씀해주세요",
    "말씀해주시겠",
    "말씀부탁",
    "선택해주시면",
    "선택해주세요",
    "골라주시면",
    "골라주세요",
    "정해주시면",
    "정해주세요",
];

/// 띄어쓰기 변형("알려 주시면" / "알려주시면")을 한 형태로 모은다 — 표기마다 항목을 늘리면
/// 목록이 두 배가 되고 빠뜨린 변형이 생긴다.
fn asks_for_answer(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    ASKING_PHRASES.iter().any(|phrase| compact.contains(phrase))
}

/// 마지막 실질 줄이 물음표로 끝나거나, 답을 달라는 정중 요청을 담고 있으면 답 대기로 본다.
///
/// 한국어 종결어미(`~까요`/`~나요`) 전체로 넓히지는 않는다 — "이대로 진행하겠습니다"류의 완료
/// 보고를 질문으로 오인하기 시작한다. 요청 표현을 열거하는 쪽이 경계가 분명하다.
pub fn is_question(text: &str) -> bool {
    match last_substantive_line(text).map(strip_trailing_ornament) {
        Some(line) => line.ends_with('?') || line.ends_with('？') || asks_for_answer(line),
        None => false,
    }
}

/// 턴 에필로그에서 모은 신호 — 데스크톱(`commands.rs`)과 러너(`runner/process.rs`)가 같은 것을 본다.
#[derive(Clone, Copy)]
pub struct TurnEpilogue<'a> {
    pub last_text: &'a str,
    /// 이 턴이 워크트리를 실제로 바꿨는가. 바꿨다면 승인받을 변경이 있다는 뜻이다.
    pub worktree_changed: bool,
    /// `Result`로 정상 마감됐는가 — 워치독 kill이나 프로세스 즉사면 false.
    pub result_seen: bool,
    pub result_error: bool,
}

/// 이 턴이 "사용자 답을 기다리는 상태"로 끝났는가.
///
/// 이전에는 **툴을 하나라도 쓰면** 무조건 검토 대기로 뒀는데, 실제 세션은 답하기 전에 거의 항상
/// 코드베이스를 읽으므로 이 게이트가 사실상 전부를 막았다(운영 DB의 검토 대기 12건 전부 NULL).
/// 판단 기준을 **워크트리 변경 여부**로 바꾼다 — 읽기만 한 턴에는 승인할 변경이 없으니, 원래
/// 경계하던 오판("승인해야 할 변경을 질문으로 표시")이 성립하지 않는다.
pub fn awaits_answer(epilogue: &TurnEpilogue<'_>) -> bool {
    epilogue.result_seen
        && !epilogue.result_error
        && !epilogue.worktree_changed
        && is_question(epilogue.last_text)
}

#[cfg(test)]
mod tests {
    use super::{awaits_answer, is_question, TurnEpilogue};

    /// 답을 요구하지만 물음표가 없는 실제 형태 — 운영 DB의 검토 대기 세션에서 그대로 가져왔다.
    #[test]
    fn polite_request_without_question_mark_is_detected() {
        assert!(is_question("어느 쪽인지 알려주시면 이어서 처리하겠습니다."));
        assert!(is_question("어느 쪽으로 할지 알려 주시면 처리하겠습니다. 참고로 다른 표는 정상입니다."));
        assert!(is_question(
            "**Q1/Q2/Q3만 답해 주시면** (예: \"B / a+b / C\") 바로 설계 문서를 쓰겠습니다. 별말씀 없으면 추천안으로 진행합니다."
        ));
        assert!(is_question("MVP는 a + b로 보는데, 우선순위 다르면 말씀해 주세요."));
    }

    /// 승인 요청은 질문이 아니다 — 이 경계가 무너지면 완료 보고가 전부 답 대기로 넘어온다.
    #[test]
    fn approval_request_is_not_an_answer_request() {
        assert!(!is_question("작업을 완료했습니다. 검토해 주세요."));
        assert!(!is_question("PR을 올렸습니다. 확인 부탁드립니다."));
        // "선택"과 "주세요"가 한 줄에 있지만 이어진 요청이 아니다 — 조각 검사였다면 걸렸을 형태.
        assert!(!is_question("선택지는 아래와 같습니다. 결과를 확인해 주세요."));
    }

    #[test]
    fn reading_the_codebase_does_not_disqualify_a_question() {
        // 툴을 썼어도 워크트리가 그대로면 승인할 변경이 없다 — 이전 게이트가 막던 지점.
        let asked = TurnEpilogue {
            last_text: "A안과 B안 중 어느 쪽으로 갈까요?",
            worktree_changed: false,
            result_seen: true,
            result_error: false,
        };
        assert!(awaits_answer(&asked));

        let edited = TurnEpilogue {
            worktree_changed: true,
            ..asked
        };
        assert!(!awaits_answer(&edited));
    }

    #[test]
    fn abnormal_turn_end_is_never_a_question() {
        let base = TurnEpilogue {
            last_text: "이대로 진행할까요?",
            worktree_changed: false,
            result_seen: true,
            result_error: false,
        };
        assert!(!awaits_answer(&TurnEpilogue { result_seen: false, ..base }));
        assert!(!awaits_answer(&TurnEpilogue { result_error: true, ..base }));
    }

    #[test]
    fn plain_question_is_detected() {
        assert!(is_question("A안과 B안 중 어느 쪽으로 갈까요?"));
        assert!(is_question("Should I proceed with the migration?"));
    }

    #[test]
    fn completion_report_is_not_a_question() {
        assert!(!is_question("작업을 완료했습니다. 검토해 주세요."));
        assert!(!is_question("3개 파일을 수정했습니다:\n- a.rs\n- b.rs"));
    }

    #[test]
    fn question_followed_by_options_is_detected() {
        // 실제로 가장 잦은 형태 — 질문 뒤에 선택지가 붙어 마지막 줄이 불릿이 된다.
        let text = "어느 방식으로 구현할까요?\n\n- A: 새 상태 추가\n- B: 컬럼 주석\n";
        assert!(is_question(text));
    }

    #[test]
    fn question_followed_by_code_block_is_detected() {
        let text = "이 시그니처로 바꿀까요?\n\n```rust\nfn f() {}\n```\n";
        assert!(is_question(text));
    }

    #[test]
    fn emphasis_around_question_is_stripped() {
        assert!(is_question("**정말 이대로 진행할까요?**"));
        assert!(is_question("(계속 진행할까요?)"));
    }

    #[test]
    fn fullwidth_question_mark_is_detected() {
        assert!(is_question("계속할까요？"));
    }

    #[test]
    fn rhetorical_question_mid_text_is_ignored() {
        // 본문 중간의 의문문은 답을 요구하지 않는다 — 마지막 실질 줄만 본다.
        let text = "왜 실패했을까요? 원인은 락 경합이었습니다. 수정 후 테스트가 통과합니다.";
        assert!(!is_question(text));
        let multi = "왜 실패했을까요?\n\n원인은 락 경합이었고, 수정 후 통과합니다.";
        assert!(!is_question(multi));
    }

    #[test]
    fn empty_or_blank_text_is_not_a_question() {
        assert!(!is_question(""));
        assert!(!is_question("\n\n   \n"));
        assert!(!is_question("```\ncode\n```"));
    }

    #[test]
    fn numbered_options_after_question_are_skipped() {
        let text = "어떤 순서로 진행할까요?\n1. 백엔드 먼저\n2. 프런트 먼저";
        assert!(is_question(text));
    }
}
