//! 턴이 `Result` 없이 끝났을 때의 사인(死因) 분류.
//!
//! 문자열 하나로는 소비처가 "자동 재시도해도 되는가"를 판단할 수 없다. 인터럽트와
//! 유휴 타임아웃은 사람이 읽는 문구만 다르고 타입은 같아서, 구분하려면 문구를 파싱해야 했다.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    Interrupted,
    IdleTimeout,
    NoProgress,
    ProcessDied,
    PendingWorkers,
}

/// 소비처가 행동을 고르는 축. UI 상태와 1:1로 대응시킨다.
/// OpenHuman은 여기에 *정책 차단* 카테고리를 하나 더 두지만 Praxis의 이 경로에는
/// 정책 게이트가 없어 3분류로 충분하다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    Recoverable,
    NeedsUserAction,
    UserDeclined,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurnFailure {
    pub class: FailureClass,
    pub category: FailureCategory,
    pub cause_plain: String,
    pub next_action: &'static str,
}

impl FailureClass {
    fn category(self) -> FailureCategory {
        match self {
            Self::Interrupted => FailureCategory::UserDeclined,
            Self::IdleTimeout | Self::PendingWorkers => FailureCategory::Recoverable,
            Self::NoProgress | Self::ProcessDied => FailureCategory::NeedsUserAction,
        }
    }

    fn next_action(self) -> &'static str {
        match self {
            Self::Interrupted => "필요하면 같은 지시를 다시 보내세요",
            Self::IdleTimeout => "그대로 이어서 보내면 세션이 재개됩니다",
            Self::NoProgress => "반복 실패한 도구 호출의 전제를 바꿔 다시 지시하세요",
            Self::ProcessDied => "디스크 여유 공간과 벤더 인증 상태를 확인하세요",
            Self::PendingWorkers => "이어서 보내 남은 Worker 결과를 회수하세요",
        }
    }
}

impl TurnFailure {
    fn new(class: FailureClass, cause_plain: String) -> Self {
        Self {
            class,
            category: class.category(),
            cause_plain,
            next_action: class.next_action(),
        }
    }

    pub fn pending_workers(titles: &[String]) -> Self {
        Self::new(
            FailureClass::PendingWorkers,
            format!(
                "백그라운드 Worker {}개를 회수하기 전에 턴이 끝났습니다 — {}",
                titles.len(),
                titles.join(", ")
            ),
        )
    }
}

impl super::TurnOutcome {
    /// 사인 판정의 **단일 진입점**. IDE·러너 두 호출 경로가 이 함수만 쓴다.
    /// 우선순위: 인터럽트 > 브레이커 halt > 유휴 타임아웃 > 프로세스 즉사.
    pub fn classify(&self, interrupted: bool, idle_timeout_secs: u64) -> TurnFailure {
        if interrupted {
            return TurnFailure::new(
                FailureClass::Interrupted,
                "턴이 중단되었습니다 (사용자 인터럽트)".to_string(),
            );
        }
        if let Some(summary) = &self.halted {
            return TurnFailure::new(FailureClass::NoProgress, summary.clone());
        }
        if self.timed_out {
            return TurnFailure::new(
                FailureClass::IdleTimeout,
                format!(
                    "턴이 유휴 타임아웃으로 중단되었습니다 ({} 무출력)",
                    super::humanize_secs(idle_timeout_secs)
                ),
            );
        }
        let mut cause = format!(
            "에이전트 프로세스가 결과 없이 종료되었습니다 ({})",
            self.exit_desc
        );
        let tail = self.stderr_tail.trim();
        if tail.is_empty() {
            cause.push_str(" — 디스크 여유 공간/인증 상태를 확인하세요");
        } else {
            cause.push_str("\n\nstderr:\n");
            cause.push_str(tail);
        }
        TurnFailure::new(FailureClass::ProcessDied, cause)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convo::TurnOutcome;

    fn outcome(timed_out: bool, exit_desc: &str, stderr_tail: &str) -> TurnOutcome {
        TurnOutcome {
            session_id: "sid".into(),
            timed_out,
            exit_desc: exit_desc.into(),
            stderr_tail: stderr_tail.into(),
            halted: None,
        }
    }

    #[test]
    fn interrupt_wins_over_every_other_signal() {
        let mut out = outcome(true, "signal 9", "");
        out.halted = Some("같은 호출 3회 실패".into());
        let failure = out.classify(true, 14_400);
        assert_eq!(failure.class, FailureClass::Interrupted);
        assert_eq!(failure.category, FailureCategory::UserDeclined);
    }

    #[test]
    fn breaker_halt_outranks_process_death() {
        // halt는 스스로 kill_group을 부르므로 exit_desc가 "signal 9"로 남는다.
        // 뒤로 밀면 자기가 만든 신호를 프로세스 즉사로 오분류한다.
        let mut out = outcome(false, "signal 9", "");
        out.halted = Some("Bash npm test 3회 연속 실패".into());
        let failure = out.classify(false, 14_400);
        assert_eq!(failure.class, FailureClass::NoProgress);
        assert_eq!(failure.category, FailureCategory::NeedsUserAction);
    }

    #[test]
    fn idle_timeout_renders_humanized_limit() {
        let failure = outcome(true, "signal 9", "").classify(false, 14_400);
        assert_eq!(failure.class, FailureClass::IdleTimeout);
        assert_eq!(failure.category, FailureCategory::Recoverable);
        assert!(failure.cause_plain.contains("4시간"));
    }

    #[test]
    fn process_death_surfaces_stderr_tail_when_present() {
        let failure = outcome(false, "exit 3", "boom: no space left").classify(false, 14_400);
        assert_eq!(failure.class, FailureClass::ProcessDied);
        assert!(failure.cause_plain.contains("exit 3"));
        assert!(failure.cause_plain.contains("no space left"));
    }

    #[test]
    fn process_death_without_stderr_falls_back_to_hint() {
        let failure = outcome(false, "exit 1", "  ").classify(false, 14_400);
        assert!(failure.cause_plain.contains("디스크 여유 공간"));
    }

    #[test]
    fn pending_workers_is_recoverable_by_resume() {
        let failure = TurnFailure::pending_workers(&["구현 Worker".into()]);
        assert_eq!(failure.class, FailureClass::PendingWorkers);
        assert_eq!(failure.category, FailureCategory::Recoverable);
        assert!(failure.cause_plain.contains("구현 Worker"));
    }
}
