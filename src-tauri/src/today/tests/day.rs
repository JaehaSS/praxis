use crate::today::day::{local_day, next_day, prev_day, KST_OFFSET_SECS};

#[test]
fn utc_evening_is_already_next_day_in_kst() {
    // 2026-08-02T15:00:00Z = 2026-08-03T00:00:00+09:00 → KST로는 이미 3일
    assert_eq!(local_day(1785682800, KST_OFFSET_SECS).unwrap(), "2026-08-03");
}

#[test]
fn one_second_before_kst_midnight_is_still_previous_day() {
    assert_eq!(local_day(1785682799, KST_OFFSET_SECS).unwrap(), "2026-08-02");
}

#[test]
fn utc_offset_zero_gives_utc_calendar_day() {
    assert_eq!(local_day(1785682800, 0).unwrap(), "2026-08-02");
}

#[test]
fn prev_and_next_cross_month_boundary() {
    assert_eq!(prev_day("2026-08-01").unwrap(), "2026-07-31");
    assert_eq!(next_day("2026-07-31").unwrap(), "2026-08-01");
}

#[test]
fn malformed_day_string_is_rejected() {
    assert!(prev_day("2026-13-99").is_err());
    assert!(next_day("어제").is_err());
}
