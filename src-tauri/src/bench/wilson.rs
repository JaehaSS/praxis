//! 이항 비율의 Wilson score interval.
//!
//! 정규근사(p̂ ± z·√(p̂(1-p̂)/n))를 쓰지 않는 이유는 하나다 — 이 하네스의 표본은 **작다**.
//! 태스크 5개 × 3회면 n = 15이고, 정규근사는 그 크기에서 [0,1] 밖으로 나가거나 p̂이 0/1일 때
//! 폭이 0인 구간을 내놓는다. "3전 3승이니 승률 100%"라고 말하는 셈이다. Wilson은 두 경우 모두
//! 정직한 폭을 준다.

/// 95% 신뢰수준의 z값.
const Z: f64 = 1.96;

/// (하한, 상한). `total`이 0이면 (0.0, 1.0) — "모른다"를 가장 넓은 구간으로 표현한다.
///
/// `passed > total`은 호출자의 버그지만 패닉시키지 않는다 — 관측 표시용 함수가 앱을 죽이는 것은
/// 과한 대가다. 비율을 1.0으로 클램프해 계산한다.
pub fn interval(passed: u32, total: u32) -> (f64, f64) {
    if total == 0 {
        return (0.0, 1.0);
    }
    let n = f64::from(total);
    let p = (f64::from(passed) / n).clamp(0.0, 1.0);
    let z2 = Z * Z;
    let denominator = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denominator;
    let margin = (Z / denominator) * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
    ((center - margin).max(0.0), (center + margin).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 표본이 없으면 아무것도 모른다 — 좁은 구간으로 아는 척하지 않는다.
    #[test]
    fn no_samples_means_the_widest_interval() {
        assert_eq!(interval(0, 0), (0.0, 1.0));
    }

    /// 정규근사가 폭 0을 내놓는 두 경계. Wilson은 여기서도 폭을 가진다.
    #[test]
    fn a_perfect_record_still_has_an_interval() {
        let (low, high) = interval(3, 3);
        assert!(low > 0.0 && low < 1.0, "하한이 퇴화했다: {low}");
        assert!((high - 1.0).abs() < 1e-9, "상한은 1이어야 한다: {high}");
        // 3전 3승으로 "승률 90% 이상"이라고 말할 수 없어야 한다.
        assert!(low < 0.9, "표본 3개로 구간이 너무 좁다: {low}");
    }

    #[test]
    fn a_shutout_still_has_an_interval() {
        let (low, high) = interval(0, 3);
        assert!(low.abs() < 1e-9, "하한은 0이어야 한다: {low}");
        assert!(high > 0.0 && high < 1.0, "상한이 퇴화했다: {high}");
    }

    /// p̂과 1-p̂은 서로의 거울이어야 한다.
    #[test]
    fn the_interval_is_symmetric_under_complement() {
        let (low_a, high_a) = interval(3, 10);
        let (low_b, high_b) = interval(7, 10);
        assert!((low_a - (1.0 - high_b)).abs() < 1e-9);
        assert!((high_a - (1.0 - low_b)).abs() < 1e-9);
    }

    #[test]
    fn the_interval_always_contains_the_observed_rate() {
        for total in 1u32..=30 {
            for passed in 0..=total {
                let rate = f64::from(passed) / f64::from(total);
                let (low, high) = interval(passed, total);
                assert!(
                    low <= rate + 1e-9 && rate <= high + 1e-9,
                    "{passed}/{total}: {rate}이 [{low}, {high}] 밖"
                );
            }
        }
    }

    #[test]
    fn the_interval_stays_within_zero_and_one() {
        for total in 1u32..=30 {
            for passed in 0..=total {
                let (low, high) = interval(passed, total);
                assert!((0.0..=1.0).contains(&low), "{passed}/{total} 하한 {low}");
                assert!((0.0..=1.0).contains(&high), "{passed}/{total} 상한 {high}");
                assert!(low <= high);
            }
        }
    }

    /// 표본이 늘면 구간이 좁아진다 — 이 성질이 없으면 "더 돌릴 이유"가 없어진다.
    #[test]
    fn more_samples_narrow_the_interval() {
        let (low_small, high_small) = interval(5, 10);
        let (low_big, high_big) = interval(50, 100);
        assert!(high_big - low_big < high_small - low_small);
    }

    /// 60% vs 70%를 작은 표본으로 구별할 수 없다 — 이 하네스의 기대치를 못 박는 테스트다.
    #[test]
    fn small_samples_cannot_separate_nearby_rates() {
        let (low_a, high_a) = interval(6, 10);
        let (low_b, high_b) = interval(7, 10);
        assert!(
            low_b < high_a && low_a < high_b,
            "표본 10개로 60%와 70%의 구간이 분리됐다 — 기대치 설정이 틀렸다"
        );
    }

    #[test]
    fn a_bogus_count_does_not_panic() {
        let (low, high) = interval(5, 3);
        assert!((0.0..=1.0).contains(&low) && (0.0..=1.0).contains(&high));
    }
}
