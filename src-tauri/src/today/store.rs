//! DayItem CRUD·수동 정렬. 순수 sqlx — Tauri 타입을 참조하지 않는다.

use super::{DayItem, DayStatus};
use sqlx::{Row, SqlitePool};

const COLUMNS: &str = "id, day, title, note, status, position, repo, task_id, source, \
                       source_ref, created_at, updated_at, done_at, carried_from";

fn row_to_item(row: &sqlx::sqlite::SqliteRow) -> DayItem {
    DayItem {
        id: row.get("id"),
        day: row.get("day"),
        title: row.get("title"),
        note: row.get("note"),
        status: row.get("status"),
        position: row.get("position"),
        repo: row.get("repo"),
        task_id: row.get("task_id"),
        source: row.get("source"),
        source_ref: row.get("source_ref"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        done_at: row.get("done_at"),
        carried_from: row.get("carried_from"),
    }
}

pub async fn list(pool: &SqlitePool, day: &str) -> Result<Vec<DayItem>, String> {
    let sql = format!("SELECT {COLUMNS} FROM day_items WHERE day = ? ORDER BY position, id");
    let rows = sqlx::query(&sql)
        .bind(day)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(row_to_item).collect())
}

/// 날짜 범위 조회. 경계(`from`·`to`)를 포함한다 — 캘린더가 달의 첫날·마지막날을 그대로 넘긴다.
///
/// 집계하지 않고 항목을 그대로 돌려준다. 월 최대 수백 행이라 전송 비용이 무의미하고,
/// 프론트가 날짜를 바꿀 때 재조회가 없다 (설계 0023 DR-3).
pub async fn range(pool: &SqlitePool, from: &str, to: &str) -> Result<Vec<DayItem>, String> {
    // 백로그를 **명시적으로** 뺀다. 문자열 비교상 'backlog'는 어떤 날짜보다 크므로 지금은
    // BETWEEN이 알아서 거르지만, 그 우연에 기대면 날짜 형식이나 센티넬 이름이 바뀔 때
    // 조용히 깨진다 (플랜 0054 DR-2).
    let sql = format!(
        "SELECT {COLUMNS} FROM day_items WHERE day BETWEEN ? AND ? AND day <> '{}' \
         ORDER BY day, position, id",
        super::day::BACKLOG
    );
    let rows = sqlx::query(&sql)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(row_to_item).collect())
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<DayItem, String> {
    let sql = format!("SELECT {COLUMNS} FROM day_items WHERE id = ?");
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("항목 {id}을 찾을 수 없습니다"))?;
    Ok(row_to_item(&row))
}

/// 다음 정렬 키 — 해당 날짜의 최대 position + 1 (빈 날은 0).
pub(super) async fn next_position(pool: &SqlitePool, day: &str) -> Result<i64, String> {
    let row =
        sqlx::query("SELECT COALESCE(MAX(position) + 1, 0) AS next FROM day_items WHERE day = ?")
            .bind(day)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row.get("next"))
}

pub async fn add(
    pool: &SqlitePool,
    day: &str,
    title: &str,
    repo: Option<&str>,
    now: i64,
) -> Result<DayItem, String> {
    add_sourced(pool, day, title, repo, "manual", None, now).await
}

/// 제안에서 담을 때 쓰는 경로 — `source`/`source_ref`가 붙는다. 같은 날 같은 출처는
/// unique index가 막으므로, 중복이면 Err를 그대로 올린다(호출자가 "이미 담음"으로 처리).
pub async fn add_sourced(
    pool: &SqlitePool,
    day: &str,
    title: &str,
    repo: Option<&str>,
    source: &str,
    source_ref: Option<&str>,
    now: i64,
) -> Result<DayItem, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("제목이 비었습니다".into());
    }
    // 날짜가 아니라 **레인 키**를 받는다 — 백로그에 직접 적을 수 있어야 한다. 떠오른 일을
    // 오늘에 넣었다가 다시 미는 것은 두 번 일하는 것이다.
    super::day::validate_key(day)?;
    let position = next_position(pool, day).await?;
    let result = sqlx::query(
        "INSERT INTO day_items (day, title, status, position, repo, source, source_ref, created_at, updated_at) \
         VALUES (?, ?, 'open', ?, ?, ?, ?, ?, ?)",
    )
    .bind(day)
    .bind(title)
    .bind(position)
    .bind(repo)
    .bind(source)
    .bind(source_ref)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    get(pool, result.last_insert_rowid()).await
}

/// None인 필드는 변경하지 않는다.
pub async fn update(
    pool: &SqlitePool,
    id: i64,
    title: Option<&str>,
    note: Option<&str>,
    repo: Option<&str>,
    now: i64,
) -> Result<DayItem, String> {
    if let Some(t) = title {
        if t.trim().is_empty() {
            return Err("제목이 비었습니다".into());
        }
    }
    sqlx::query(
        "UPDATE day_items SET title = COALESCE(?, title), note = COALESCE(?, note), \
         repo = COALESCE(?, repo), updated_at = ? WHERE id = ?",
    )
    .bind(title.map(str::trim))
    .bind(note)
    .bind(repo)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    get(pool, id).await
}

/// `done`으로 가면 완료 시각을 찍고, 되돌리면 지운다 — 마감 집계가 done_at을 신뢰한다.
pub async fn set_status(
    pool: &SqlitePool,
    id: i64,
    status: DayStatus,
    now: i64,
) -> Result<DayItem, String> {
    let done_at = (status == DayStatus::Done).then_some(now);
    sqlx::query("UPDATE day_items SET status = ?, done_at = ?, updated_at = ? WHERE id = ?")
        .bind(status.as_str())
        .bind(done_at)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    get(pool, id).await
}

/// 주어진 순서대로 position을 0..n으로 다시 매긴다. `day`에 속하지 않는 id는 무시한다
/// (프런트가 낡은 목록을 보낼 때 다른 날 항목이 섞여 들어오는 것을 막는다).
pub async fn reorder(
    pool: &SqlitePool,
    day: &str,
    ordered_ids: &[i64],
    now: i64,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    for (index, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE day_items SET position = ?, updated_at = ? WHERE id = ? AND day = ?")
            .bind(index as i64)
            .bind(now)
            .bind(id)
            .bind(day)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())
}

/// 항목을 다른 레인(날짜 또는 백로그)으로 옮긴다. 대상의 맨 끝에 붙는다.
///
/// **옮긴다 — 복제하지 않는다** (`carry` 헤더와 같은 규약). 같은 행이 이동하므로 `task_id`
/// 링크가 그대로 살아 있다. 이것이 백로그를 별도 테이블로 두지 않은 이유다 (플랜 0054 DR-1).
///
/// `carried_from`은 지운다 — 이월 전용 필드이고, 손으로 옮긴 것은 이월이 아니다 (DR-5).
pub async fn move_to(pool: &SqlitePool, id: i64, to: &str, now: i64) -> Result<DayItem, String> {
    super::day::validate_key(to)?;
    let item = get(pool, id).await?;
    if item.day == to {
        return Ok(item);
    }
    // 끝난 결정을 미결로 되돌리는 경로를 막는다. 되살리려면 먼저 `open`으로 돌린다.
    if item.status != DayStatus::Open.as_str() {
        return Err("완료·보류한 항목은 옮길 수 없습니다".into());
    }
    let position = next_position(pool, to).await?;
    sqlx::query(
        "UPDATE day_items SET day = ?, position = ?, carried_from = NULL, updated_at = ? \
         WHERE id = ?",
    )
    .bind(to)
    .bind(position)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    get(pool, id).await
}

pub async fn remove(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM day_items WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Task 완료 → 항목 자동 체크. 상태 전이 훅 대신 조회 시점에 대조한다 (설계 0021 DR-4).
///
/// 대상은 `status='open'` + `task_id IS NOT NULL`뿐이다 — 사람이 done/dropped로 내린
/// 결정은 덮지 않는다. `tasks`의 상태 컬럼명은 `state`다 (`db/mod.rs:105`).
pub async fn reconcile_tasks(pool: &SqlitePool, day: &str, now: i64) -> Result<(), String> {
    sqlx::query(
        "UPDATE day_items SET status = 'done', done_at = ?, updated_at = ? \
         WHERE day = ? AND status = 'open' AND task_id IS NOT NULL \
           AND task_id IN (SELECT id FROM tasks WHERE state = 'Done')",
    )
    .bind(now)
    .bind(now)
    .bind(day)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// `reconcile_tasks`의 범위판. 단일 UPDATE라 31일치도 한 번에 저렴하다.
/// 대상 조건은 단일 날짜판과 같다 — 사람이 내린 done/dropped는 덮지 않는다.
pub async fn reconcile_tasks_range(
    pool: &SqlitePool,
    from: &str,
    to: &str,
    now: i64,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE day_items SET status = 'done', done_at = ?, updated_at = ? \
         WHERE day BETWEEN ? AND ? AND day <> 'backlog' AND status = 'open' \
           AND task_id IS NOT NULL \
           AND task_id IN (SELECT id FROM tasks WHERE state = 'Done')",
    )
    .bind(now)
    .bind(now)
    .bind(from)
    .bind(to)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 항목에 착수한 Task를 연결한다. 1:1이므로 유니크 인덱스가 중복을 막는다.
pub async fn link_task(
    pool: &SqlitePool,
    id: i64,
    task_id: i64,
    now: i64,
) -> Result<(), String> {
    sqlx::query("UPDATE day_items SET task_id = ?, updated_at = ? WHERE id = ?")
        .bind(task_id)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
