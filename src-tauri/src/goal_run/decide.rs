//! 다음 틱에 무엇을 할지 정하는 순수 함수 (계획 0036 DR-1·DR-6).

use super::{exhausted, Budget, ExhaustReason, Spent};
use crate::db::state as tstate;

/// 직전 시도가 지금 어떤 처지인가. `tasks.state`에서 파생한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// 시도가 아직 없다 (Run 생성 직후).
    None,
    /// 승인 대기·실행 중·검토 대기 — 사람이나 에이전트가 아직 일하는 중이다.
    Busy,
    /// 끝났다 (Done/Failed). 이제 증거로 판정할 수 있다.
    Settled,
    /// 사람이 거부했다.
    Rejected,
}

/// `tasks.state` → `Attempt`.
///
/// `AwaitingReview`를 `Busy`로 두는 것이 중요하다 — 에이전트는 끝냈지만 **사람이 아직 안 봤다.**
/// 여기서 재진입하면 검토 중인 결과 위에 다음 시도가 쌓인다.
pub fn attempt_of_state(state: &str) -> Attempt {
    match state {
        tstate::DISCARDED => Attempt::Rejected,
        tstate::DONE | tstate::FAILED => Attempt::Settled,
        _ => Attempt::Busy,
    }
}

/// 검증 증거의 판정. `verify::GateResult::ready`에 대응한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Pass,
    Fail,
}

impl GateVerdict {
    pub fn from_ready(ready: bool) -> Self {
        if ready {
            Self::Pass
        } else {
            Self::Fail
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// 아무것도 하지 않는다.
    Wait,
    /// 같은 목표로 다음 시도를 만든다.
    Reenter,
    /// 목표 달성 — Run 종료.
    Satisfy,
    /// 예산 소진 — Run 종료.
    Exhaust(ExhaustReason),
    /// 사람이 멈췄다 — Run 종료.
    Stop(&'static str),
    /// 기계적으로 판정할 수 없다 — 사람에게 넘기고 Run 종료.
    HandOff,
}

/// 판정 우선순위: **예산 소진 > 사람의 처분 > 증거**.
///
/// 예산이 맨 앞인 이유 — 소진된 Run은 어떤 이유로도 다시 돌면 안 된다.
///
/// 사람의 처분이 증거보다 앞인 이유 — 거부는 목표 판정이 아니라 **정지 신호**다(DR-6).
/// 거부를 실패로 취급하면, 안전을 위해 승인 대기를 유지한 결정(DR-5)이 정확히 무한
/// 재생성의 원천이 된다. 사람이 "이건 아니다"라고 말한 것을 예산이 바닥날 때까지 다시 만든다.
pub fn decide(
    budget: &Budget,
    spent: &Spent,
    attempt: Attempt,
    gate: Option<GateVerdict>,
) -> Decision {
    if let Some(reason) = exhausted(budget, spent) {
        return Decision::Exhaust(reason);
    }
    match attempt {
        Attempt::Rejected => Decision::Stop("사용자가 시도를 거부했습니다"),
        Attempt::Busy => Decision::Wait,
        // 첫 시도. 예산이 남아 있으므로 만든다.
        Attempt::None => Decision::Reenter,
        Attempt::Settled => match gate {
            Some(GateVerdict::Pass) => Decision::Satisfy,
            Some(GateVerdict::Fail) => Decision::Reenter,
            // 증거가 없으면 실패로 간주해 재시도하지 않는다 — 검증 커맨드를 못 찾는
            // 프로젝트에서 이것이 무한 재시도의 원천이 된다(DR-1의 Unknown).
            None => Decision::HandOff,
        },
    }
}
