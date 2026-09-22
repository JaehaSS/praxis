//! 미완료 항목 자동 이월.
//!
//! 설계 0021은 이월을 **의도적으로 배제**하고 `carry` 제안 소스로만 반영했다(DR-3).
//! 실사용 후 번복한 결정이다 — 매일 "담기"를 누르는 비용이 제안의 값을 넘었다.
//! 경위와 대가는 설계 0021 Changelog 참조.
//!
//! 조회 시 lazy로 돈다 (`store::reconcile_tasks`와 같은 방식, DR-4). 전이 훅을 거는 대신
//! "보는 순간 정확"을 택한 것과 같은 이유다.
//!
//! **옮긴다 — 복제하지 않는다.** `idx_day_items_task`가 `task_id`에 걸린 전역 유니크라
//! 복제본은 링크를 가져올 수 없고, 그러면 착수 중인 일이 미착수로 보여 이중 착수를 부른다.

use sqlx::{Row, SqlitePool};

/// 지난 날의 `open` 항목을 전부 `day`로 옮기고, 옮긴 건수를 돌려준다.
///
/// `done`·`dropped`는 대상이 아니다 — 접은 계획("안 하기로 했다")을 되살리지 않는다.
/// **상한이 없다**: 며칠을 밀렸든 계속 따라온다. 자동 정리는 사용자 결정으로 넣지 않았다
/// (밀린 항목은 `dropped`로 직접 접는다).
///
/// 하루가 아니라 `day` **이전 전체**를 훑는다. 주말·휴가로 앱을 안 열면 하루씩 잇는 방식은
/// 그 구간에서 끊긴다.
pub async fn carry_forward(pool: &SqlitePool, day: &str, now: i64) -> Result<u64, String> {
    super::day::validate(day)?;
    // `day <> 'backlog'`를 **명시한다**. 'backlog'는 문자열 비교상 어떤 날짜보다 커서 지금은
    // `day < ?`가 알아서 거르지만, 그 우연이 깨지는 날의 증상은 "백로그 전체가 어느 아침
    // 오늘 목록에 쏟아지는 것"이다 (플랜 0054 DR-2).
    let rows = sqlx::query(
        "SELECT id, day FROM day_items \
         WHERE day < ? AND day <> 'backlog' AND status = 'open' ORDER BY day, position, id",
    )
    .bind(day)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(0);
    }

    // 오늘 항목 뒤에 붙인다. 밀린 일을 위로 올리지 않는 것은 취향 문제라 기존 추가 순서
    // 규약(`store::next_position`)을 그대로 따르고, 순서는 사용자가 ↑↓로 바꾼다.
    let mut position = super::store::next_position(pool, day).await?;
    let mut moved = 0u64;
    for row in &rows {
        let id: i64 = row.get("id");
        let from: String = row.get("day");
        let result = sqlx::query(
            "UPDATE day_items SET day = ?, carried_from = ?, position = ?, updated_at = ? \
             WHERE id = ?",
        )
        .bind(day)
        .bind(&from)
        .bind(position)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await;
        match result {
            Ok(_) => {
                position += 1;
                moved += 1;
            }
            // `idx_day_items_source`(day, source, source_ref) 위반 = 같은 출처의 일이 오늘
            // 목록에 이미 있다. 중복을 만들지 않고 지난 날에 그대로 둔다 — 한 건이 막혀도
            // 나머지 이월은 계속한다.
            Err(e) if e.to_string().contains("UNIQUE") => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(moved)
}
