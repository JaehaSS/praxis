//! 작업별 코드 그래프 빌드 취소 상태.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

const RUNNING: u8 = 0;
const CANCELLED: u8 = 1;
const PROMOTING: u8 = 2;

#[derive(Clone, Default)]
pub struct BuildJobs {
    states: Arc<Mutex<HashMap<i64, Arc<AtomicU8>>>>,
}

pub struct BuildGuard {
    task_id: i64,
    state: Arc<AtomicU8>,
    jobs: BuildJobs,
}

impl BuildJobs {
    pub fn start(&self, task_id: i64) -> Result<BuildGuard, String> {
        let mut states = self
            .states
            .lock()
            .map_err(|_| "코드 그래프 작업 잠금 실패")?;
        if states.contains_key(&task_id) {
            return Err("이 작업의 코드 그래프 인덱싱이 이미 진행 중입니다".to_string());
        }
        let state = Arc::new(AtomicU8::new(RUNNING));
        states.insert(task_id, Arc::clone(&state));
        Ok(BuildGuard {
            task_id,
            state,
            jobs: self.clone(),
        })
    }

    pub fn cancel(&self, task_id: i64) -> bool {
        let state = self
            .states
            .lock()
            .ok()
            .and_then(|states| states.get(&task_id).cloned());
        let Some(state) = state else {
            return false;
        };
        state
            .compare_exchange(RUNNING, CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}

impl BuildGuard {
    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::SeqCst) == CANCELLED
    }

    /// 승격을 시작하면 이후 취소 요청은 거부된다 — publish 선형화 지점이다.
    pub(crate) fn begin_promotion(&self) -> bool {
        self.state
            .compare_exchange(RUNNING, PROMOTING, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}

impl Drop for BuildGuard {
    fn drop(&mut self) {
        let Ok(mut states) = self.jobs.states.lock() else {
            return;
        };
        let owns_slot = states
            .get(&self.task_id)
            .is_some_and(|state| Arc::ptr_eq(state, &self.state));
        if owns_slot {
            states.remove(&self.task_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_is_visible_and_drop_releases_the_slot() {
        let jobs = BuildJobs::default();
        let guard = jobs.start(7).unwrap();
        assert!(!guard.is_cancelled());
        assert!(jobs.start(7).is_err(), "같은 작업의 빌드를 겹치면 안 된다");

        assert!(jobs.cancel(7));
        assert!(guard.is_cancelled());
        drop(guard);
        assert!(jobs.start(7).is_ok());
    }
}
