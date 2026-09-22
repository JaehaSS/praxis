use std::time::Instant;

use super::model::{ResultEnvelope, PREVIEW_PROBE_TASK_ID, PREVIEW_PROBE_WEBVIEW_LABEL};
use super::registry::PreviewBridge;
use super::validation::is_command_id;

impl PreviewBridge {
    pub fn start_probe(&self) {
        *self
            .probe_started
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Instant::now());
    }

    pub fn probe_elapsed_ms(&self) -> Option<u128> {
        self.probe_started
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|started| started.elapsed().as_millis())
    }

    pub fn is_completed_probe(&self, caller: &str, result: &ResultEnvelope) -> bool {
        if result.task_id != PREVIEW_PROBE_TASK_ID || caller != PREVIEW_PROBE_WEBVIEW_LABEL {
            return false;
        }
        self.lock_sessions()
            .get(&result.task_id)
            .is_some_and(|state| {
                state.registration.webview_label == caller
                    && state.completed_command.as_deref() == Some(result.command_id.as_str())
                    && is_command_id(&result.command_id)
            })
    }
}
