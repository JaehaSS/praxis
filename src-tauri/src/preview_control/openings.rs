//! UI와 MCP의 창 생성을 작업별로 예약한다. 네이티브 호출을 잠금 안에서 하지 않는다.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::preview_bridge::mcp::DispatchError;

#[derive(Clone, Default)]
pub struct PreviewOpenings(Arc<Mutex<HashMap<i64, (u64, bool)>>>);

pub struct Opening {
    openings: PreviewOpenings,
    task_id: i64,
    pub generation: u64,
}

impl PreviewOpenings {
    pub fn begin(&self, task_id: i64, generation: u64) -> Result<Opening, DispatchError> {
        let mut slots = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if slots.contains_key(&task_id) {
            return Err(DispatchError::Busy);
        }
        slots.insert(task_id, (generation, false));
        Ok(Opening {
            openings: self.clone(),
            task_id,
            generation,
        })
    }

    /// 취소와 핸들 제거를 같은 잠금으로 묶어 등록과의 경합을 막는다.
    pub fn close<T>(&self, task_id: i64, remove: impl FnOnce() -> T) -> T {
        let mut slots = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((_, cancelled)) = slots.get_mut(&task_id) {
            *cancelled = true;
        }
        remove()
    }

    /// 등록 전에 사용자가 새 창을 닫은 경우에도 생성을 취소한다.
    pub fn cancel_generation(&self, task_id: i64, generation: u64) {
        let mut slots = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((current, cancelled)) = slots.get_mut(&task_id) {
            if *current == generation {
                *cancelled = true;
            }
        }
    }

    pub fn close_if_current(&self, task_id: i64, remove: impl FnOnce() -> bool) -> bool {
        let mut slots = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let removed = remove();
        if removed {
            if let Some((_, cancelled)) = slots.get_mut(&task_id) {
                *cancelled = true;
            }
        }
        removed
    }
}

impl Opening {
    /// 등록만 직렬화한다. 클로저 안에서 네이티브 API를 호출하지 않는다.
    pub fn publish<T>(&self, publish: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let slots = self
            .openings
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if slots.get(&self.task_id) != Some(&(self.generation, false)) {
            return Err("preview_open_cancelled".into());
        }
        publish()
    }
}

impl Drop for Opening {
    fn drop(&mut self) {
        let mut slots = self
            .openings
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if slots
            .get(&self.task_id)
            .is_some_and(|(generation, _)| *generation == self.generation)
        {
            slots.remove(&self.task_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_task_is_busy_other_task_can_open_and_drop_allows_retry() {
        let slots = PreviewOpenings::default();
        let first = slots.begin(1, 10).unwrap();
        assert!(matches!(slots.begin(1, 11), Err(DispatchError::Busy)));
        let other = slots.begin(2, 12).unwrap();
        assert!(other.publish(|| Ok(())).is_ok());
        drop(first);
        assert!(slots.begin(1, 13).is_ok());
    }

    #[test]
    fn close_cancels_unpublished_window_and_does_not_release_its_slot_early() {
        let slots = PreviewOpenings::default();
        let opening = slots.begin(1, 10).unwrap();
        slots.close(1, || ());
        assert_eq!(
            opening.publish(|| panic!("cancelled window published")),
            Err::<(), _>("preview_open_cancelled".into())
        );
        assert!(matches!(slots.begin(1, 11), Err(DispatchError::Busy)));
        drop(opening);
        assert!(slots.begin(1, 12).is_ok());
    }

    #[test]
    fn stale_window_close_cannot_cancel_a_new_generation() {
        let slots = PreviewOpenings::default();
        let opening = slots.begin(1, 11).unwrap();
        slots.cancel_generation(1, 10);
        assert!(opening.publish(|| Ok(())).is_ok());
        slots.cancel_generation(1, 11);
        assert!(opening.publish(|| Ok(())).is_err());
    }
}
