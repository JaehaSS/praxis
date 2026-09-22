//! Bearer 토큰 맵. 토큰 하나가 task 하나에 묶이고, 턴·task가 끝나면 폐기된다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::preview_bridge::random_hex_id;

/// 폐기를 놓쳐도 토큰이 하루를 넘기지 못하게 하는 백스톱.
const TOKEN_TTL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct TokenEntry {
    pub task_id: i64,
    pub spawn_id: String,
    pub issued_at: Instant,
}

#[derive(Clone, Default)]
pub struct ControlTokens(
    Arc<Mutex<HashMap<String, TokenEntry>>>,
    Arc<Mutex<HashMap<(i64, String), usize>>>,
);

/// Retained by the dispatch task even if the HTTP caller disconnects.
pub struct CallLease {
    pub task_id: i64,
    spawn_id: String,
    active: Arc<Mutex<HashMap<(i64, String), usize>>>,
}
impl Drop for CallLease {
    fn drop(&mut self) {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        let key = (self.task_id, self.spawn_id.clone());
        if let Some(count) = active.get_mut(&key) {
            *count -= 1;
            if *count == 0 { active.remove(&key); }
        }
    }
}

impl ControlTokens {
    pub fn issue(&self, task_id: i64, spawn_id: &str) -> Result<String, String> {
        let token = random_hex_id()?;
        self.lock().insert(
            token.clone(),
            TokenEntry {
                task_id,
                spawn_id: spawn_id.to_string(),
                issued_at: Instant::now(),
            },
        );
        Ok(token)
    }

    pub fn task_for(&self, token: &str) -> Option<i64> {
        self.task_for_at(token, Instant::now())
    }

    /// 시계를 인자로 받아 TTL 경계를 테스트할 수 있게 한다.
    pub fn task_for_at(&self, token: &str, now: Instant) -> Option<i64> {
        let mut tokens = self.lock();
        let entry = tokens.get(token)?;
        if now.saturating_duration_since(entry.issued_at) >= TOKEN_TTL {
            tokens.remove(token);
            return None;
        }
        Some(entry.task_id)
    }

    pub fn acquire(&self, token: &str) -> Option<CallLease> {
        let mut tokens = self.lock();
        let entry = tokens.get(token)?;
        if entry.issued_at.elapsed() >= TOKEN_TTL { tokens.remove(token); return None; }
        let lease = CallLease { task_id: entry.task_id, spawn_id: entry.spawn_id.clone(), active: self.1.clone() };
        // The token lock stays held until accounting is visible to revocation.
        *self.1.lock().unwrap_or_else(|e| e.into_inner()).entry((lease.task_id, lease.spawn_id.clone())).or_default() += 1;
        Some(lease)
    }

    pub fn active_for_task(&self, task_id: i64) -> usize {
        self.1.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|((task, _), _)| *task == task_id).map(|(_, count)| *count).sum()
    }

    pub fn active_for_spawn(&self, spawn_id: &str) -> usize {
        self.1.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|((_, spawn), _)| spawn == spawn_id).map(|(_, count)| *count).sum()
    }

    pub fn revoke(&self, token: &str) {
        self.lock().remove(token);
    }

    /// 턴 종료: 그 spawn이 받은 토큰만 죽인다.
    pub fn revoke_spawn(&self, spawn_id: &str) {
        self.lock().retain(|_, entry| entry.spawn_id != spawn_id);
    }

    /// task 종결: 그 task로 향하는 토큰을 모두 죽인다.
    pub fn revoke_task(&self, task_id: i64) {
        self.lock().retain(|_, entry| entry.task_id != task_id);
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, TokenEntry>> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_lives_until_the_ttl_boundary() {
        let tokens = ControlTokens::default();
        let token = tokens.issue(42, "spawn-1").unwrap();
        let almost = Instant::now() + TOKEN_TTL - Duration::from_secs(1);

        assert_eq!(tokens.task_for_at(&token, almost), Some(42));
    }

    #[test]
    fn token_is_gone_at_and_after_the_ttl() {
        let tokens = ControlTokens::default();
        let token = tokens.issue(42, "spawn-1").unwrap();
        let expiry = Instant::now() + TOKEN_TTL;

        assert_eq!(tokens.task_for_at(&token, expiry), None);
        // 만료는 항목을 지운다 — 시계를 되돌려도 되살아나지 않는다.
        assert_eq!(tokens.task_for(&token), None);
    }
}

#[cfg(test)]
mod drain_tests {
    use super::*;
    #[test]
    fn revoke_closes_admission_while_existing_dispatch_stays_owned(){
        let tokens=ControlTokens::default();let first=tokens.issue(1,"one").unwrap();let second=tokens.issue(2,"two").unwrap();
        let lease=tokens.acquire(&first).unwrap();let other=tokens.acquire(&second).unwrap();
        tokens.revoke(&first);assert!(tokens.acquire(&first).is_none());assert_eq!(tokens.active_for_task(1),1);
        assert_eq!(tokens.active_for_spawn("one"),1);drop(lease);assert_eq!(tokens.active_for_task(1),0);
        assert_eq!(tokens.active_for_task(2),1);assert!(tokens.task_for(&second).is_some());drop(other);assert_eq!(tokens.active_for_task(2),0);
    }
}
