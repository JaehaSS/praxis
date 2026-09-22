//! 토론 세션의 순수 도메인 — 프롬프트 접미 조립·합의 판정·라운드 상태.
//!
//! 데스크톱(`commands.rs`)과 러너(`runner/process.rs`)의 턴 조립이 서로 다르므로 공유하는 것은
//! 조립 전체가 아니라 **접미**와 **판정**이다(설계 §4-3). 부작용은 호출부에 남기고 여기에는
//! 문자열과 카운터만 둔다 — 그래야 두 경로의 종료 판정이 갈리지 않는다.

use std::ops::RangeInclusive;

use super::{DebateEndReason, Side};

/// 합의 선언 마커. 판정은 **마지막 줄 정확 일치**만이다 — 지시문이 상대 발화 인용을 요구하므로
/// `contains`는 인용문 속 마커에 오탐한다(설계 §3).
pub const CONSENSUS_MARKER: &str = "[합의]";

/// 기본 라운드 상한. 벤더 호출 6회이고, 사용자가 개입 없이 기다릴 만한 상한이 그 정도다(Q2).
pub const DEFAULT_ROUND_CAP: u32 = 3;

/// 설정으로 조절 가능한 범위. 2는 반박 한 번으로 끝나고, 5 이상은 직접 묻는 것보다 비싸진다.
pub const ROUND_CAP_RANGE: RangeInclusive<u32> = 2..=5;

/// 매 턴 같은 문자열로 붙는 토론 규약(설계 §3 지시문 초안 원문).
/// 마지막 문장(쓰기 금지)이 토론 중 파일 변경을 막는 **유일한** 장치다(Q6).
pub const DEBATE_PROTOCOL: &str = "당신은 다른 AI 에이전트와 함께 하나의 작업을 두고 토론하고 있습니다. 목표는 이기는 것이 아니라 **사용자가 채택할 수 있는 하나의 결론**에 이르는 것입니다. 상대의 직전 발화를 인용해 어디에 동의하고 어디에 반대하는지 먼저 밝히고, 반대에는 근거(코드 경로·측정값·실패 사례)를 붙이십시오. 근거 없는 선호는 반대가 아닙니다. 상대의 지적이 옳으면 즉시 인정하고 자기 안을 고치십시오 — 입장을 지키는 것은 이 자리의 목적이 아닙니다. 남은 이견이 사용자의 결정을 바꾸지 않을 만큼 작아졌다고 판단하면, 합의된 결론을 3줄 이내로 요약한 뒤 마지막 줄에 정확히 `[합의]`만 적으십시오. 이견이 남았다면 마커를 적지 말고, 무엇이 남았는지 한 줄로 밝히십시오. 파일을 수정하지 말고 명령을 실행하지 마십시오 — 이 라운드에서 당신이 하는 일은 판단뿐입니다.";

/// 이번 턴의 프롬프트 접미에 들어갈 재료. 자리(좌·우)는 바뀌지 않으므로 고지하지 않는다(Q5).
pub struct RoundContext<'a> {
    pub round: u32,
    pub cap: u32,
    pub opponent_agent: &'a str,
    pub user_message: &'a str,
    /// 상대의 **직전 하나**만 중계한다. 전체 로그를 붙이면 라운드마다 컨텍스트가 선형으로 분다.
    pub opponent_last: Option<&'a str>,
}

/// 규약 → 라운드 헤더 → 사용자 발화 → 상대 직전 발화. 캡슐은 붙이지 않는다 —
/// 호출부가 이미 맨 앞에 둔다(ADR 0170).
pub fn debate_suffix(ctx: &RoundContext) -> String {
    let mut out = format!(
        "{DEBATE_PROTOCOL}\n\n## 라운드 {}/{} · 상대: {}\n\n## 사용자 발화\n{}\n",
        ctx.round, ctx.cap, ctx.opponent_agent, ctx.user_message
    );
    if let Some(last) = ctx.opponent_last {
        out.push_str("\n## 상대 직전 발화\n");
        out.push_str(last);
        out.push('\n');
    }
    out
}

/// 우측의 첫 턴 — 새로 판 세션이라 작업 맥락이 없다. 에이전트 전환이 쓰는 인계 조립을 앞에
/// 두고 접미를 뒤에 붙인다. 새 조립 파이프라인을 만들지 않는다(설계 §3).
pub fn first_right_prompt(handoff: &str, ctx: &RoundContext) -> String {
    format!("{handoff}\n{}", debate_suffix(ctx))
}

/// 마지막 **비어 있지 않은** 줄이 마커와 정확히 일치할 때만 참.
pub fn is_consensus(last_text: &str) -> bool {
    last_text
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        == Some(CONSENSUS_MARKER)
}

/// 설정값 → 라운드 상한. 미설정·파싱 실패·범위 밖은 기본값이다 — 저장 시점에 거부하므로
/// 여기까지 온 이상한 값은 손으로 고친 DB뿐이고, 그때 토론을 못 열게 할 이유가 없다.
pub fn round_cap_from_setting(value: Option<&str>) -> u32 {
    value
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|cap| ROUND_CAP_RANGE.contains(cap))
        .unwrap_or(DEFAULT_ROUND_CAP)
}

/// 한 턴이 끝난 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcome {
    Spoke { consensus: bool },
    Failed,
    Interrupted,
}

/// 다음에 할 일 — 반대편 턴이거나 종료다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Next(Side),
    End(DebateEndReason),
}

/// 라운드 장부. 합의는 라운드 경계가 아니라 **연속한 두 턴**으로 판정하므로 불리언 하나가 아니라
/// 카운터 하나면 된다(설계 §3).
#[derive(Debug, Clone, Copy)]
pub struct DebateState {
    round: u32,
    cap: u32,
    consecutive_markers: u8,
}

impl DebateState {
    pub fn new(cap: u32) -> Self {
        Self {
            round: 1,
            cap,
            consecutive_markers: 0,
        }
    }

    pub fn round(&self) -> u32 {
        self.round
    }

    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// 턴 하나를 장부에 넣고 다음 걸음을 돌려준다.
    pub fn advance(&mut self, side: Side, outcome: TurnOutcome) -> Step {
        let consensus = match outcome {
            // 성공한 쪽 발화는 남기고 경계만 닫는다.
            TurnOutcome::Failed => return Step::End(DebateEndReason::Error),
            TurnOutcome::Interrupted => return Step::End(DebateEndReason::Aborted),
            TurnOutcome::Spoke { consensus } => consensus,
        };
        self.consecutive_markers = if consensus {
            self.consecutive_markers.saturating_add(1)
        } else {
            0
        };
        if self.consecutive_markers >= 2 {
            return Step::End(DebateEndReason::Consensus);
        }
        if side == Side::Right {
            if self.round >= self.cap {
                return Step::End(DebateEndReason::RoundCap);
            }
            self.round += 1;
        }
        Step::Next(side.opposite())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(opponent_last: Option<&'a str>) -> RoundContext<'a> {
        RoundContext {
            round: 2,
            cap: 3,
            opponent_agent: "codex",
            user_message: "이 설계가 맞나?",
            opponent_last,
        }
    }

    fn spoke(consensus: bool) -> TurnOutcome {
        TurnOutcome::Spoke { consensus }
    }

    /// (1) 인용문·본문 중간의 마커는 합의가 아니다. 마지막 줄 단독일 때만 참이다.
    #[test]
    fn consensus_matches_only_the_last_nonempty_line() {
        assert!(is_consensus("결론 세 줄\n[합의]"));
        assert!(is_consensus("결론\n[합의]  \n\n"));
        assert!(!is_consensus("상대가 \"[합의]\"라고 했지만 이견이 남았다"));
        assert!(!is_consensus("[합의]\n아직 하나 남았다"));
        assert!(!is_consensus("[합의] 하겠습니다"));
        assert!(!is_consensus(""));
    }

    /// (2) 한쪽만 마커를 내면 계속 돈다 — 동의는 상대의 응답으로만 확정된다.
    #[test]
    fn single_marker_keeps_going() {
        let mut state = DebateState::new(3);
        assert_eq!(state.advance(Side::Left, spoke(true)), Step::Next(Side::Right));
        assert_eq!(state.advance(Side::Right, spoke(false)), Step::Next(Side::Left));
        assert_eq!(state.round(), 2);
    }

    /// (3) 연속한 두 턴이 모두 마커면 합의다.
    #[test]
    fn consecutive_markers_end_in_consensus() {
        let mut state = DebateState::new(3);
        assert_eq!(state.advance(Side::Left, spoke(true)), Step::Next(Side::Right));
        assert_eq!(
            state.advance(Side::Right, spoke(true)),
            Step::End(DebateEndReason::Consensus)
        );
    }

    /// (4) R이 먼저 동의하고 다음 L이 동의하면 라운드 경계 전에 끝난다(반 라운드 조기 종료).
    #[test]
    fn marker_pair_across_round_boundary_ends_early() {
        let mut state = DebateState::new(3);
        state.advance(Side::Left, spoke(false));
        assert_eq!(state.advance(Side::Right, spoke(true)), Step::Next(Side::Left));
        assert_eq!(state.round(), 2);
        assert_eq!(
            state.advance(Side::Left, spoke(true)),
            Step::End(DebateEndReason::Consensus)
        );
    }

    /// (5) 마커 없이 상한 라운드의 R 턴이 끝나면 상한 종료다.
    #[test]
    fn round_cap_ends_after_last_right_turn() {
        for cap in [2, 3] {
            let mut state = DebateState::new(cap);
            for round in 1..cap {
                assert_eq!(state.advance(Side::Left, spoke(false)), Step::Next(Side::Right));
                assert_eq!(state.advance(Side::Right, spoke(false)), Step::Next(Side::Left));
                assert_eq!(state.round(), round + 1);
            }
            assert_eq!(state.advance(Side::Left, spoke(false)), Step::Next(Side::Right));
            assert_eq!(
                state.advance(Side::Right, spoke(false)),
                Step::End(DebateEndReason::RoundCap)
            );
        }
    }

    /// (6) 턴 실패는 오류로, 사용자 중단은 중단으로 끝난다.
    #[test]
    fn failure_and_interrupt_end_the_debate() {
        let mut state = DebateState::new(3);
        assert_eq!(
            state.advance(Side::Left, TurnOutcome::Failed),
            Step::End(DebateEndReason::Error)
        );
        let mut state = DebateState::new(3);
        assert_eq!(
            state.advance(Side::Right, TurnOutcome::Interrupted),
            Step::End(DebateEndReason::Aborted)
        );
    }

    /// (7) 접미 순서는 규약 → 헤더 → 사용자 발화 → 상대 발화이고, 상대 발화가 없으면 그 절이 빠진다.
    #[test]
    fn suffix_keeps_its_order_and_drops_the_missing_clause() {
        let with_last = debate_suffix(&ctx(Some("나는 반대다")));
        let protocol = with_last.find(DEBATE_PROTOCOL).expect("규약");
        let header = with_last.find("## 라운드 2/3 · 상대: codex").expect("헤더");
        let user = with_last.find("## 사용자 발화").expect("사용자 발화");
        let opponent = with_last.find("## 상대 직전 발화").expect("상대 발화");
        assert!(protocol < header && header < user && user < opponent);
        assert!(with_last.contains("나는 반대다"));

        let without_last = debate_suffix(&ctx(None));
        assert!(!without_last.contains("## 상대 직전 발화"));
        assert!(without_last.contains("이 설계가 맞나?"));
    }

    /// (8) 자리는 바뀌지 않으므로 헤더가 좌우를 고지하지 않는다 — 고지하면 사라진 뒤 거짓이 된다.
    #[test]
    fn header_does_not_announce_a_side() {
        let suffix = debate_suffix(&ctx(None));
        let header = suffix
            .lines()
            .find(|line| line.starts_with("## 라운드"))
            .expect("헤더");
        for banned in ["좌", "우", "left", "right"] {
            assert!(!header.contains(banned), "헤더가 자리를 고지한다: {header}");
        }
    }

    /// (9) 우측 첫 턴은 인계 조립이 접미보다 앞이다 — 맥락이 먼저 와야 규약이 읽힌다.
    #[test]
    fn first_right_prompt_puts_handoff_first() {
        let prompt = first_right_prompt("## Cross-agent handoff\n요약", &ctx(Some("직전 발화")));
        assert!(prompt.starts_with("## Cross-agent handoff"));
        assert!(prompt.find("Cross-agent handoff").unwrap() < prompt.find(DEBATE_PROTOCOL).unwrap());
    }

    #[test]
    fn round_cap_setting_falls_back_to_the_default() {
        assert_eq!(round_cap_from_setting(None), DEFAULT_ROUND_CAP);
        assert_eq!(round_cap_from_setting(Some("nope")), DEFAULT_ROUND_CAP);
        assert_eq!(round_cap_from_setting(Some("7")), DEFAULT_ROUND_CAP);
        assert_eq!(round_cap_from_setting(Some("4")), 4);
    }
}
