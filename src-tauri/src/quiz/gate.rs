//! 대기가 충분히 길어졌는지 판정한다 (설계 0044 DR-2).
//!
//! **예측하지 않고 관찰한다.** 대기가 시작되는 순간에는 얼마나 걸릴지 알 수 없으므로,
//! 이미 임계값을 넘긴 대기에만 퀴즈를 올린다. 에이전트 작업 시간이 heavy-tail 분포라
//! 경과 시간 자체가 가장 강한 예측 신호이고, 추가 계산 없이 얻어진다.
//!
//! 단위는 **초**다 — `ActiveConvo.started_at`이 `commands::now()`(초)로 채워지기 때문이다.
//! 밀리초로 두면 임계값이 1000배로 부풀어 게이트가 영영 열리지 않는다.
//!
//! Tauri 비의존 — 시각을 인자로 받으므로 `cargo test`로 직접 검증된다.

/// 이 시간을 넘긴 대기에만 퀴즈를 띄운다.
///
/// 25초에 실측 근거는 없다(설계 0044 Unknowns 1). 실제 대기 분포를 측정한 뒤 조정할 것.
pub const DEFAULT_THRESHOLD_SECS: i64 = 25;

/// `started_at_secs`는 **이번 턴의 시작 시각**이다 — 마지막 이벤트 시각이 아니다.
/// 후자를 쓰면 도구를 쉬지 않고 돌리는 긴 작업에서 게이트가 영영 열리지 않는다.
pub fn should_offer(now_secs: i64, started_at_secs: i64, threshold_secs: i64) -> bool {
    now_secs.saturating_sub(started_at_secs) >= threshold_secs
}

#[cfg(test)]
mod tests {
    use super::{should_offer, DEFAULT_THRESHOLD_SECS};

    #[test]
    fn opens_only_after_the_threshold() {
        assert!(!should_offer(1, 0, 25), "1초 경과에 열렸다");
        assert!(!should_offer(24, 0, 25), "임계값 직전에 열렸다");
        assert!(should_offer(25, 0, 25), "임계값 정각에 안 열렸다");
        assert!(should_offer(60, 0, 25));
    }

    /// 시계가 뒤로 가면 경과가 음수가 된다. 그때 게이트가 열리면 짧은 대기에도 퀴즈가 뜬다.
    #[test]
    fn clock_skew_does_not_open_the_gate() {
        assert!(!should_offer(0, 10, 25));
        assert!(!should_offer(-5, 0, 25));
    }

    /// 임계값 0은 "항상 연다"는 뜻이어야 한다 — 수동 테스트에서 기다리지 않으려고 쓴다.
    #[test]
    fn zero_threshold_always_opens() {
        assert!(should_offer(0, 0, 0));
    }

    /// 초 단위여야 한다. 밀리초로 두면 25000초(약 7시간)가 되어 게이트가 안 열린다.
    #[test]
    fn default_threshold_is_25_seconds() {
        assert_eq!(DEFAULT_THRESHOLD_SECS, 25);
        assert!(should_offer(30, 0, DEFAULT_THRESHOLD_SECS));
    }
}
