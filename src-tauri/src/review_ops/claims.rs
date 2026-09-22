use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct ReviewClaims {
    entries: Arc<Mutex<HashMap<i64, TaskClaims>>>,
}

#[derive(Default)]
struct TaskClaims {
    verify: bool,
    finalization: bool,
}

#[derive(Clone, Copy)]
enum ClaimKind {
    Verify,
    Finalization,
}

pub struct ReviewClaim {
    claims: ReviewClaims,
    task_id: i64,
    kind: ClaimKind,
}

impl ReviewClaims {
    pub fn claim_verify(&self, task_id: i64) -> Result<ReviewClaim, String> {
        self.claim(task_id, ClaimKind::Verify)
    }

    pub fn claim_finalization(&self, task_id: i64) -> Result<ReviewClaim, String> {
        self.claim(task_id, ClaimKind::Finalization)
    }

    fn claim(&self, task_id: i64, kind: ClaimKind) -> Result<ReviewClaim, String> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let current = entries.entry(task_id).or_default();
        if !current.can_claim(kind) {
            return Err(claim_error(kind).to_string());
        }
        current.set(kind, true);
        Ok(ReviewClaim {
            claims: self.clone(),
            task_id,
            kind,
        })
    }

    fn release(&self, task_id: i64, kind: ClaimKind) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(current) = entries.get_mut(&task_id) else {
            return;
        };
        current.set(kind, false);
        if !current.verify && !current.finalization {
            entries.remove(&task_id);
        }
    }
}

impl TaskClaims {
    fn can_claim(&self, kind: ClaimKind) -> bool {
        match kind {
            ClaimKind::Verify => !self.verify && !self.finalization,
            ClaimKind::Finalization => !self.verify && !self.finalization,
        }
    }

    fn set(&mut self, kind: ClaimKind, value: bool) {
        match kind {
            ClaimKind::Verify => self.verify = value,
            ClaimKind::Finalization => self.finalization = value,
        }
    }
}

impl Drop for ReviewClaim {
    fn drop(&mut self) {
        self.claims.release(self.task_id, self.kind);
    }
}

fn claim_error(kind: ClaimKind) -> &'static str {
    match kind {
        ClaimKind::Verify => "이미 검증 중이거나 작업 종료 처리 중입니다",
        ClaimKind::Finalization => "검증이 진행 중입니다",
    }
}
