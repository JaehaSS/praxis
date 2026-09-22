use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::oneshot;

use super::model::{
    CancelReason, PendingAction, RejectReason, ResultEnvelope, SessionRegistration, SubmitOutcome,
    MAX_RESULT_BYTES,
};
use super::validation::{clear_for_navigation, no_pending_reason, sha256_hex, validate_identity};

#[derive(Clone, Default)]
pub struct PreviewBridge {
    sessions: Arc<Mutex<HashMap<i64, SessionState>>>,
    pub(super) probe_started: Arc<Mutex<Option<Instant>>>,
}

pub(super) struct SessionState {
    pub(super) registration: SessionRegistration,
    pub(super) pending: Option<PendingAction>,
    pub(super) waiter: Option<oneshot::Sender<String>>,
    pub(super) taken_over: bool,
    pub(super) fallback_command: Option<String>,
    pub(super) completed_command: Option<String>,
    pub(super) expected_navigation: bool,
    pub(super) last_cancel: Option<CancelReason>,
}

impl PreviewBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, registration: SessionRegistration) -> Result<(), RejectReason> {
        let mut sessions = self.lock_sessions();
        sessions.insert(
            registration.task_id,
            SessionState {
                registration,
                pending: None,
                waiter: None,
                taken_over: false,
                fallback_command: None,
                completed_command: None,
                expected_navigation: false,
                last_cancel: None,
            },
        );
        Ok(())
    }

    pub fn begin(&self, pending: PendingAction) -> Result<oneshot::Receiver<String>, RejectReason> {
        let mut sessions = self.lock_sessions();
        let state = sessions
            .get_mut(&pending.task_id)
            .ok_or(RejectReason::TaskMismatch)?;
        if state.registration.session_id != pending.session_id {
            return Err(RejectReason::SessionMismatch);
        }
        if state.registration.generation != pending.generation {
            return Err(RejectReason::GenerationMismatch);
        }
        if state.taken_over {
            return Err(RejectReason::TakenOver);
        }
        if state.pending.is_some() {
            return Err(RejectReason::Busy);
        }
        let (waiter, receiver) = oneshot::channel();
        state.waiter = Some(waiter);
        state.pending = Some(pending);
        state.completed_command = None;
        state.expected_navigation = false;
        state.last_cancel = None;
        Ok(receiver)
    }

    pub fn submit(
        &self,
        caller: &str,
        result: ResultEnvelope,
    ) -> Result<SubmitOutcome, RejectReason> {
        let body = result.body.as_bytes();
        if body.len() > MAX_RESULT_BYTES {
            return Err(RejectReason::PayloadTooLarge);
        }
        if serde_json::from_str::<serde_json::Value>(&result.body).is_err() {
            return Err(RejectReason::InvalidJson);
        }
        if sha256_hex(body) != result.sha256 {
            return Err(RejectReason::ShaMismatch);
        }
        let mut sessions = self.lock_sessions();
        let state = sessions
            .get_mut(&result.task_id)
            .ok_or(RejectReason::TaskMismatch)?;
        validate_identity(state, caller, &result)?;
        let pending = state
            .pending
            .as_ref()
            .ok_or_else(|| no_pending_reason(state, &result))?;
        if pending.command_id != result.command_id {
            return Err(RejectReason::CommandMismatch);
        }
        state.pending = None;
        state.fallback_command = None;
        state.completed_command = Some(result.command_id.clone());
        if let Some(waiter) = state.waiter.take() {
            let _ = waiter.send(result.body.clone());
        }
        Ok(SubmitOutcome::Accepted {
            task_id: result.task_id,
            command_id: result.command_id,
        })
    }

    pub fn begin_fallback_assembly(
        &self,
        task_id: i64,
        command_id: &str,
    ) -> Result<(), RejectReason> {
        let mut sessions = self.lock_sessions();
        let state = sessions
            .get_mut(&task_id)
            .ok_or(RejectReason::TaskMismatch)?;
        let pending = state.pending.as_ref().ok_or(RejectReason::NoPending)?;
        if pending.command_id != command_id {
            return Err(RejectReason::CommandMismatch);
        }
        state.fallback_command = Some(command_id.into());
        Ok(())
    }

    pub fn on_navigation(&self, task_id: i64, generation: u64) {
        let mut sessions = self.lock_sessions();
        let Some(state) = sessions.get_mut(&task_id) else {
            return;
        };
        clear_for_navigation(state, generation, false);
    }

    pub fn prepare_navigation(&self, task_id: i64) -> Result<u64, RejectReason> {
        let mut sessions = self.lock_sessions();
        let state = sessions
            .get_mut(&task_id)
            .ok_or(RejectReason::TaskMismatch)?;
        let generation = state.registration.generation.saturating_add(1);
        clear_for_navigation(state, generation, true);
        Ok(generation)
    }

    pub fn cancel_prepared_navigation(&self, task_id: i64) -> Result<(), RejectReason> {
        let mut sessions = self.lock_sessions();
        let state = sessions
            .get_mut(&task_id)
            .ok_or(RejectReason::TaskMismatch)?;
        state.expected_navigation = false;
        Ok(())
    }

    pub fn observe_navigation(&self, task_id: i64) -> Result<u64, RejectReason> {
        let mut sessions = self.lock_sessions();
        let state = sessions
            .get_mut(&task_id)
            .ok_or(RejectReason::TaskMismatch)?;
        if std::mem::take(&mut state.expected_navigation) {
            return Ok(state.registration.generation);
        }
        let generation = state.registration.generation.saturating_add(1);
        clear_for_navigation(state, generation, false);
        Ok(generation)
    }

    pub fn close(&self, task_id: i64) {
        self.lock_sessions().remove(&task_id);
    }

    pub fn has_pending(&self, task_id: i64) -> bool {
        self.lock_sessions()
            .get(&task_id)
            .is_some_and(|state| state.pending.is_some())
    }

    pub fn has_fallback_assembly(&self, task_id: i64) -> bool {
        self.lock_sessions()
            .get(&task_id)
            .is_some_and(|state| state.fallback_command.is_some())
    }

    pub fn is_empty(&self, task_id: i64) -> bool {
        !self.lock_sessions().contains_key(&task_id)
    }

    pub(super) fn lock_sessions(&self) -> std::sync::MutexGuard<'_, HashMap<i64, SessionState>> {
        self.sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}
