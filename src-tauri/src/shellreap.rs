//! 유휴 워크스페이스 셸 회수 (ADR 0163 결정 4).
//!
//! 셸은 패널을 닫아도 백엔드에 남는다 — 재진입 때 스크롤백을 그대로 복원하기 위해서다.
//! 그 대가로 쓰지 않는 셸이 쌓이므로, **아무도 보고 있지 않고 아무것도 돌지 않는** 셸만
//! 골라 회수한다. 세 조건을 동시에 만족해야 한다.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::pty::PtySession;

/// 회수까지의 유휴 시간.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// 유휴 스캔 간격.
pub const SCAN_INTERVAL: Duration = Duration::from_secs(60);

/// 회수된 자리에 새 셸을 열 때 스크롤백 앞에 실어 보내는 안내.
///
/// 빈 화면만 주면 사용자는 히스토리를 잃은 것인지 그냥 새 셸인지 구분할 수 없다.
pub const REAPED_NOTICE: &str =
    "\x1b[2m오래 쓰지 않아 정리된 터미널입니다 — 새 셸을 엽니다.\x1b[0m\r\n";

/// 워크스페이스 셸 하나와, 그것을 회수해도 되는지 판정하는 데 필요한 관측.
pub struct ShellSlot {
    pub session: PtySession,
    /// 이 셸을 화면에 띄우고 있는 xterm의 수. 도크와 우측 탭이 같은 셸을 쓰므로 1을 넘을 수
    /// 있다(App이 도크를 우선해 실제로는 하나지만, 카운트가 그 정책에 기대지 않게 둔다).
    attached: u32,
    /// 회수 후보가 된 시각. 조건이 깨지면 `None`으로 되돌아간다 — 유휴는 누적이 아니라
    /// 연속이어야 한다.
    idle_since: Option<Instant>,
}

impl ShellSlot {
    pub fn new(session: PtySession) -> Self {
        Self {
            session,
            attached: 0,
            idle_since: None,
        }
    }

    /// 화면이 붙었다. 붙는 순간 유휴 관측은 무효다.
    pub fn attach(&mut self) {
        self.attached = self.attached.saturating_add(1);
        self.idle_since = None;
    }

    /// 화면이 떨어졌다. `shell_open`이 완료되기 전에 컴포넌트가 사라지면 attach 없이 detach가
    /// 올 수 있으므로 0 아래로 내려가지 않게 막는다.
    pub fn detach(&mut self) {
        self.attached = self.attached.saturating_sub(1);
    }

    pub fn is_attached(&self) -> bool {
        self.attached > 0
    }
}

/// 한 셸의 유휴 관측을 갱신하고 회수 여부를 답한다.
///
/// `at_prompt`가 `None`이면(판정 불가 플랫폼) 회수하지 않는다.
pub fn observe_idle(
    idle_since: &mut Option<Instant>,
    attached: bool,
    at_prompt: Option<bool>,
    now: Instant,
    timeout: Duration,
) -> bool {
    if attached || at_prompt != Some(true) {
        *idle_since = None;
        return false;
    }
    match *idle_since {
        None => {
            *idle_since = Some(now);
            false
        }
        Some(since) => now.duration_since(since) >= timeout,
    }
}

/// 유휴 셸을 죽이고 맵에서 뺀다. 반환값은 회수된 작업 id들 — 호출자가 재진입 안내에 쓴다.
pub fn reap(slots: &mut HashMap<i64, ShellSlot>, now: Instant) -> Vec<i64> {
    let mut reaped = Vec::new();
    slots.retain(|id, slot| {
        let at_prompt = slot.session.at_prompt();
        let attached = slot.is_attached();
        if observe_idle(&mut slot.idle_since, attached, at_prompt, now, IDLE_TIMEOUT) {
            slot.session.terminate();
            reaped.push(*id);
            return false;
        }
        true
    });
    reaped
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_secs(600);

    #[test]
    fn 붙어_있는_셸은_회수하지_않는다() {
        let mut idle = None;
        let now = Instant::now();
        assert!(!observe_idle(&mut idle, true, Some(true), now, T));
        assert!(idle.is_none(), "붙어 있는 동안은 유휴 관측이 시작되지 않는다");
    }

    #[test]
    fn 전경에서_뭔가_돌면_회수하지_않는다() {
        let mut idle = None;
        let now = Instant::now();
        assert!(!observe_idle(&mut idle, false, Some(false), now, T));
        assert!(idle.is_none());
    }

    #[test]
    fn 판정할_수_없는_플랫폼에서는_회수하지_않는다() {
        let mut idle = None;
        let now = Instant::now();
        assert!(!observe_idle(&mut idle, false, None, now, T));
        assert!(idle.is_none());
    }

    #[test]
    fn 조건이_이어진_채로_시간이_차면_회수한다() {
        let mut idle = None;
        let start = Instant::now();
        assert!(!observe_idle(&mut idle, false, Some(true), start, T));
        assert!(!observe_idle(&mut idle, false, Some(true), start + T / 2, T));
        assert!(observe_idle(&mut idle, false, Some(true), start + T, T));
    }

    #[test]
    fn 중간에_명령이_돌면_시계가_처음부터_다시_간다() {
        let mut idle = None;
        let start = Instant::now();
        observe_idle(&mut idle, false, Some(true), start, T);
        // 9분째에 사용자가 명령을 돌렸다 — 유휴는 누적이 아니라 연속이어야 한다.
        observe_idle(&mut idle, false, Some(false), start + T - Duration::from_secs(60), T);
        assert!(idle.is_none());
        assert!(!observe_idle(
            &mut idle,
            false,
            Some(true),
            start + T + Duration::from_secs(60),
            T
        ));
    }

    #[test]
    fn 다시_붙으면_시계가_멈춘다() {
        let mut idle = None;
        let start = Instant::now();
        observe_idle(&mut idle, false, Some(true), start, T);
        observe_idle(&mut idle, true, Some(true), start + T / 2, T);
        assert!(idle.is_none());
        assert!(!observe_idle(&mut idle, false, Some(true), start + T, T));
    }

}
