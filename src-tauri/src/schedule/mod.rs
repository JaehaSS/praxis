//! 크론 스케줄러(Phase 3) — cron 식 due 판정. Tauri 비의존, 순수 함수 위주로 테스트 가능하게 분리.
//! 틱 루프 오케스트레이션(AppHandle 의존)은 `runner` 서브모듈.

pub mod runner;

use std::str::FromStr;

use chrono::{FixedOffset, TimeZone, Utc};
use cron::Schedule;
use sqlx::SqlitePool;

const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS schedules (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  label           TEXT NOT NULL,
  cron            TEXT NOT NULL,
  kind            TEXT NOT NULL,
  payload         TEXT NOT NULL,
  enabled         INTEGER NOT NULL DEFAULT 1,
  last_run_at     INTEGER,
  created_at      INTEGER NOT NULL,
  run_at          INTEGER,
  tz_offset_secs  INTEGER NOT NULL DEFAULT 32400
);
"#;

/// 스케줄(schedules) 테이블 마이그레이션.
pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    let mut connection = pool.acquire().await?;
    sqlx::query(MIGRATION).execute(&mut *connection).await?;
    // 구버전 DB 보강 — 신규 DB는 `MIGRATION`이 이미 만들었으므로 둘 다 no-op이다.
    // run_at: Some이면 1회성 리마인더(절대 epoch초에 1회 발화), None이면 기존 cron 반복.
    // tz_offset_secs: 크론 달력 기준 오프셋, 기본값 KST(+09:00 = 32400초).
    for column in [
        "run_at INTEGER",
        "tz_offset_secs INTEGER NOT NULL DEFAULT 32400",
    ] {
        crate::db::add_column_if_missing(&mut *connection, "schedules", column).await?;
    }
    Ok(())
}

/// 첫 부팅에서 한 번 심는 주간 회고 스케줄 (ADR 2026-09-13).
const RETRO_SEED_FLAG: &str = "retro_schedule_seeded";
const RETRO_SEED_LABEL: &str = "주간 회고";
/// 월요일 09:00 (초 분 시 일 월 요일).
const RETRO_SEED_CRON: &str = "0 0 9 * * Mon";
const RETRO_SEED_PAYLOAD: &str = r#"{"repo":"","agent":""}"#;

/// 주간 회고 스케줄을 **최초 1회만** 심는다. 실제로 삽입했으면 `Ok(true)`.
///
/// 플래그가 이미 있으면 아무것도 하지 않는다 — 사용자가 지운 스케줄이 다음 부팅에 되살아나면
/// 삭제가 의미를 잃는다. 읽기 실패(`Err`)를 "미설정"으로 접지 않는 이유도 같다: DB가 일시적으로
/// 실패한 부팅에서 지운 스케줄이 되살아난다(`lib.rs`의 `reflect_enabled`와 같은 판단).
pub async fn seed_weekly_retro(pool: &SqlitePool, tz_offset_secs: i32) -> Result<bool, String> {
    match crate::db::get_setting(pool, RETRO_SEED_FLAG).await {
        Ok(Some(_)) => return Ok(false),
        Ok(None) => {}
        Err(e) => return Err(format!("{RETRO_SEED_FLAG} 조회 실패: {e}")),
    }

    let existing = crate::db::list_schedules(pool)
        .await
        .map_err(|e| format!("스케줄 목록 조회 실패: {e}"))?;
    let already = existing.iter().any(|s| s.kind == "retro");

    let inserted = if already {
        false
    } else {
        crate::db::insert_schedule(
            pool,
            RETRO_SEED_LABEL,
            RETRO_SEED_CRON,
            "retro",
            RETRO_SEED_PAYLOAD,
            crate::now(),
            tz_offset_secs,
        )
        .await
        .map_err(|e| format!("주간 회고 스케줄 삽입 실패: {e}"))?;
        true
    };

    // 삽입 여부와 무관하게 플래그를 남긴다 — "한 번 판단했다"가 기록되어야 재시도가 없다.
    crate::db::set_setting(pool, RETRO_SEED_FLAG, "true")
        .await
        .map_err(|e| format!("{RETRO_SEED_FLAG} 기록 실패: {e}"))?;
    Ok(inserted)
}

/// `base`(직전 실행 시각, 없었으면 스케줄 생성 시각) 이후 ~ `now` 사이에 도래한 cron 실행
/// 시각이 하나라도 있으면 true. timezone 오프셋을 적용해 로컬 달력 기준으로 매칭.
///
/// 함정(M4): 과거 due 슬롯 개수를 세거나 몰아치지 않는다 — "1회라도 도래했으면 true"만
/// 반환하고, 틱당 스케줄당 최대 1회 실행 보장은 호출자(크론 틱 루프)의 책임이다.
pub fn is_due(cron_expr: &str, base: i64, now: i64, tz_offset_secs: i32) -> Result<bool, String> {
    let schedule = Schedule::from_str(cron_expr).map_err(|e| e.to_string())?;
    let offset = FixedOffset::east_opt(tz_offset_secs).ok_or("invalid timezone offset")?;
    let after = epoch_to_tz(base, offset)?;
    let now_tz = epoch_to_tz(now, offset)?;
    Ok(schedule
        .after(&after)
        .next()
        .is_some_and(|next| next <= now_tz))
}

fn epoch_to_utc(secs: i64) -> Result<chrono::DateTime<Utc>, String> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .ok_or_else(|| format!("invalid epoch seconds: {secs}"))
}

fn epoch_to_tz(secs: i64, tz: FixedOffset) -> Result<chrono::DateTime<FixedOffset>, String> {
    let utc = epoch_to_utc(secs)?;
    Ok(utc.with_timezone(&tz))
}

/// Cron 표현식의 다음 N개 실행 시각을 계산 (timezone 적용).
/// 현재 시각으로부터 최대 5개 시점을 반환 (형식: "YYYY-MM-DD HH:mm:ss").
pub fn cron_next_runs(
    cron_expr: &str,
    tz_offset_secs: i32,
    count: usize,
) -> Result<Vec<String>, String> {
    let schedule = Schedule::from_str(cron_expr).map_err(|e| e.to_string())?;
    let offset = FixedOffset::east_opt(tz_offset_secs).ok_or("invalid timezone offset")?;
    let now_utc = Utc::now();
    let now_tz = now_utc.with_timezone(&offset);

    let result = schedule
        .after(&now_tz)
        .take(count)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .collect();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// 2024-01-01 00:00:00 UTC.
    const BASE: i64 = 1704067200;

    static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 임시 파일 DB인 이유는 `sqlite::memory:`가 풀의 커넥션마다 별개의 빈 DB를 보기 때문이다
    /// (`retro/tests.rs:8`과 같은 관례).
    async fn test_pool() -> SqlitePool {
        let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = crate::testtmp::dir().join(format!(
            "praxis-schedule-{}-{sequence}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
            .await
            .unwrap();
        // 시드가 읽고 쓰는 표만 세운다 — 앱 전체 스키마를 끌어오면 무관한 변경에 깨진다.
        sqlx::raw_sql(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE tasks (id INTEGER PRIMARY KEY AUTOINCREMENT, repo TEXT NOT NULL, \
               created_at INTEGER NOT NULL);",
        )
        .execute(&pool)
        .await
        .unwrap();
        migrate(&pool).await.unwrap();
        pool
    }

    async fn retro_count(pool: &SqlitePool) -> usize {
        crate::db::list_schedules(pool)
            .await
            .unwrap()
            .iter()
            .filter(|s| s.kind == "retro")
            .count()
    }

    #[test]
    fn seeded_retro_cron_is_parseable() {
        // 시드 값이 cron 크레이트 6필드 파서를 통과하지 못하면 스케줄러가 매 틱 실패한다.
        assert!(Schedule::from_str(RETRO_SEED_CRON).is_ok());
    }

    #[tokio::test]
    async fn seeding_twice_leaves_a_single_schedule() {
        let pool = test_pool().await;
        assert_eq!(seed_weekly_retro(&pool, 32400).await, Ok(true));
        assert_eq!(seed_weekly_retro(&pool, 32400).await, Ok(false));
        assert_eq!(retro_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn deleted_retro_schedule_is_not_resurrected() {
        let pool = test_pool().await;
        seed_weekly_retro(&pool, 32400).await.unwrap();
        let id = crate::db::list_schedules(&pool).await.unwrap()[0].id;
        crate::db::remove_schedule(&pool, id).await.unwrap();

        assert_eq!(seed_weekly_retro(&pool, 32400).await, Ok(false));
        assert_eq!(retro_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn latest_task_repo_is_none_without_tasks_and_newest_with_them() {
        let pool = test_pool().await;
        assert_eq!(crate::db::latest_task_repo(&pool).await.unwrap(), None);

        for (repo, created_at) in [("/old", BASE), ("/new", BASE + 60)] {
            sqlx::query("INSERT INTO tasks (repo, created_at) VALUES (?, ?)")
                .bind(repo)
                .bind(created_at)
                .execute(&pool)
                .await
                .unwrap();
        }
        assert_eq!(
            crate::db::latest_task_repo(&pool).await.unwrap().as_deref(),
            Some("/new")
        );
    }

    #[test]
    fn every_minute_is_due_after_one_minute_elapsed() {
        // "0 * * * * *" = 매분 0초. BASE+60이 다음 실행 시각과 일치.
        const TZ: i32 = 0; // UTC
        assert_eq!(is_due("0 * * * * *", BASE, BASE + 60, TZ), Ok(true));
    }

    #[test]
    fn every_minute_not_due_within_same_minute() {
        const TZ: i32 = 0;
        assert_eq!(is_due("0 * * * * *", BASE, BASE + 30, TZ), Ok(false));
    }

    #[test]
    fn every_hour_on_the_hour_is_due_after_one_hour() {
        // "0 0 * * * *" = 매시 정각.
        const TZ: i32 = 0;
        assert_eq!(is_due("0 0 * * * *", BASE, BASE + 3600, TZ), Ok(true));
    }

    #[test]
    fn every_hour_not_due_within_same_hour() {
        const TZ: i32 = 0;
        assert_eq!(is_due("0 0 * * * *", BASE, BASE + 1800, TZ), Ok(false));
    }

    #[test]
    fn future_only_schedule_is_not_yet_due() {
        // 연 단위로 먼 미래(2099년)만 남은 식(cron 크레이트 지원 상한 2100) — now가 그 이전이면 false.
        const TZ: i32 = 0;
        assert_eq!(is_due("0 0 0 1 1 * 2099", BASE, BASE + 3600, TZ), Ok(false));
    }

    #[test]
    fn invalid_cron_expr_is_err() {
        const TZ: i32 = 0;
        assert!(is_due("not a cron expr", BASE, BASE + 60, TZ).is_err());
    }

    #[test]
    fn base_from_last_run_prevents_duplicate_execution_in_same_slot() {
        // base가 이미 현재 슬롯(now) 시각과 같으면(호출자가 last_run_at을 base로 전달) "다음"
        // 슬롯은 아직 안 왔으므로 false — M4, 중복 실행 방지의 핵심 불변식.
        const TZ: i32 = 0;
        assert_eq!(
            is_due("0 * * * * *", BASE + 3600, BASE + 3600, TZ),
            Ok(false)
        );
    }
}
