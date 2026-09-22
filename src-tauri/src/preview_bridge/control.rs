//! Agent control surface: cancelling an in-flight command and taking the
//! preview back from the agent. Split from `registry.rs` to keep that file
//! within the module's 200-line limit.

use super::model::CancelReason;
use super::registry::PreviewBridge;

impl PreviewBridge {
    /// 에이전트의 이동은 사용자 회수와 진행 중 액션을 침범하지 않는다.
    pub fn prepare_agent_navigation(&self, task_id: i64) -> Result<u64, super::RejectReason> {
        let mut sessions = self.lock_sessions();
        let state = sessions
            .get_mut(&task_id)
            .ok_or(super::RejectReason::TaskMismatch)?;
        if state.taken_over {
            return Err(super::RejectReason::TakenOver);
        }
        if state.pending.is_some() {
            return Err(super::RejectReason::Busy);
        }
        let generation = state.registration.generation.saturating_add(1);
        super::validation::clear_for_navigation(state, generation, true);
        Ok(generation)
    }

    /// Drops the waiter's sender, which makes the caller awaiting the receiver observe a cancel.
    pub fn cancel(&self, task_id: i64, command_id: &str) {
        let mut sessions = self.lock_sessions();
        let Some(state) = sessions.get_mut(&task_id) else {
            return;
        };
        if state
            .pending
            .as_ref()
            .map(|pending| pending.command_id.as_str())
            != Some(command_id)
        {
            return;
        }
        state.pending = None;
        if state.waiter.take().is_some() {
            state.last_cancel = Some(CancelReason::Cancel);
        }
    }

    pub fn take_over(&self, task_id: i64) {
        let mut sessions = self.lock_sessions();
        let Some(state) = sessions.get_mut(&task_id) else {
            return;
        };
        state.taken_over = true;
        state.pending = None;
        if state.waiter.take().is_some() {
            state.last_cancel = Some(CancelReason::TakeOver);
        }
    }

    /// 대기자가 끊긴 이유를 한 번만 돌려준다 — 읽는 쪽이 오류 이름을 고르고 나면 소비된다.
    pub fn take_cancel_reason(&self, task_id: i64) -> Option<CancelReason> {
        let mut sessions = self.lock_sessions();
        sessions.get_mut(&task_id)?.last_cancel.take()
    }

    pub fn release(&self, task_id: i64) {
        let mut sessions = self.lock_sessions();
        if let Some(state) = sessions.get_mut(&task_id) {
            state.taken_over = false;
        }
    }

    pub fn is_taken_over(&self, task_id: i64) -> bool {
        self.lock_sessions()
            .get(&task_id)
            .is_some_and(|state| state.taken_over)
    }

    pub fn session_of(&self, task_id: i64) -> Option<(String, u64)> {
        self.lock_sessions().get(&task_id).map(|state| {
            (
                state.registration.session_id.clone(),
                state.registration.generation,
            )
        })
    }
}
