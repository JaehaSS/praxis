//! 로컬 달력일 계산. `schedule` 모듈의 tz 오프셋 규약(기본 KST=32400)을 따른다
//! (`schedule/mod.rs:23`).

use chrono::{Duration, FixedOffset, NaiveDate, TimeZone, Utc};

/// 기본 오프셋 — `schedules.tz_offset_secs`의 DEFAULT와 같은 값.
pub const KST_OFFSET_SECS: i32 = 32400;

/// epoch초 → 해당 오프셋 기준 'YYYY-MM-DD'.
pub fn local_day(epoch_secs: i64, tz_offset_secs: i32) -> Result<String, String> {
    let offset = FixedOffset::east_opt(tz_offset_secs).ok_or("invalid timezone offset")?;
    let utc = Utc
        .timestamp_opt(epoch_secs, 0)
        .single()
        .ok_or_else(|| format!("invalid epoch seconds: {epoch_secs}"))?;
    Ok(utc.with_timezone(&offset).format("%Y-%m-%d").to_string())
}

fn parse(day: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").map_err(|e| format!("잘못된 날짜 '{day}': {e}"))
}

pub fn prev_day(day: &str) -> Result<String, String> {
    Ok((parse(day)? - Duration::days(1))
        .format("%Y-%m-%d")
        .to_string())
}

pub fn next_day(day: &str) -> Result<String, String> {
    Ok((parse(day)? + Duration::days(1))
        .format("%Y-%m-%d")
        .to_string())
}

/// 커맨드 인자 검증용 — 'YYYY-MM-DD' 형식이고 실재하는 날짜인지 확인한다.
pub fn validate(day: &str) -> Result<(), String> {
    parse(day).map(|_| ())
}

/// 백로그 레인의 `day` 값 — 날짜가 아니다 (플랜 0054 DR-1).
///
/// `validate`는 이 값을 **거부한다**. 날짜를 받아야 하는 자리(마감·계획 캘린더)에 백로그가
/// 새어 들어가는 것을 그 비대칭이 막는다. 레인 키를 받는 자리에서는 `validate_key`를 쓴다.
pub const BACKLOG: &str = "backlog";

pub fn is_backlog(key: &str) -> bool {
    key == BACKLOG
}

/// `day_items.day`에 들어갈 수 있는 값인지 — 날짜이거나 백로그이거나.
pub fn validate_key(key: &str) -> Result<(), String> {
    if is_backlog(key) {
        return Ok(());
    }
    validate(key)
}
