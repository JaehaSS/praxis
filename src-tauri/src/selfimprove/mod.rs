//! 자기개선 (Phase 5) — L1 반성. Tauri 비의존(반성 생성은 capture 측).
//! (L2 플레이북은 설계 0008에서 제거 — 상수만 있고 진입점 없던 죽은 개념)
//!
//! 안전 원칙(PRD): 자기개선 변경은 **자동 적용 금지**. 항상 검토 가능한 제안(proposal)으로
//! 저장하고, 사용자가 적용(apply) 또는 거부(reject)한다. 적용된 제안도 검증 전에는 후보 지식일 뿐이다.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::memory::{self, tier};

pub mod status {
    pub const PROPOSED: &str = "proposed";
    pub const APPLIED: &str = "applied";
    pub const REJECTED: &str = "rejected";
    /// 적용을 되무른 상태. 연결된 후보 지식은 archive로 함께 보낸다.
    pub const WITHDRAWN: &str = "withdrawn";
}

pub mod pkind {
    pub const REFLECTION: &str = "reflection"; // L1
    /// 스킬 본문 제안. 적용하면 SKILL.md가 쓰이고, 철회하면 스냅샷으로 되돌아간다.
    ///
    /// 진입점(생성 경로)과 왕복이 **함께** 들어왔다 — 상수만 두지 않는다. 이 모듈의 첫 줄에
    /// 그렇게 죽은 개념의 기록이 남아 있다(계획 0038 DR-2).
    pub const SKILL: &str = "skill";
}

/// 스킬 본문 상한. `skills/mod.rs`의 `MAX_SKILL_BYTES`와 같은 값이다 — 저장 시점에 막지 않으면
/// 적용 시점에야 걸려, 사람이 검토를 마친 뒤에 거부당한다.
const MAX_SKILL_BYTES: usize = 64 * 1024;

/// 본문에 있으면 안 되는 마커. 투영이 이 문자열로 블록 경계를 잡으므로, 스킬 본문에 섞이면
/// projector가 그 위치를 경계로 오인한다(원장 #28).
const RESERVED_MARKERS: [&str; 3] = [
    "<!-- praxis:begin -->",
    "<!-- praxis:capsule begin -->",
    "PRAXIS MEMORY",
];

/// 스킬 제안이 적용 가능한 형태인지 본다.
///
/// 저장 시점에 검사하는 이유는 하나다 — 제안은 **사람이 검토한 뒤에** 적용된다. 적용 시점에만
/// 막으면 검토를 마치고 적용을 눌렀을 때 거부당하고, 그 시점에는 고칠 방법이 없다.
fn validate_skill_body(target_path: &str, body: &str) -> anyhow::Result<()> {
    if target_path.trim().is_empty() {
        anyhow::bail!("대상 경로가 없는 스킬 제안은 적용할 곳이 없습니다");
    }
    if body.len() > MAX_SKILL_BYTES {
        anyhow::bail!(
            "스킬 본문이 64KB 한도를 초과합니다 ({}바이트)",
            body.len()
        );
    }
    if let Some(marker) = RESERVED_MARKERS.iter().find(|m| body.contains(**m)) {
        anyhow::bail!("스킬 본문에 예약 마커 '{marker}'가 있습니다 — 투영 블록 경계를 오인시킵니다");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Proposal {
    pub id: i64,
    pub repo: String,
    pub kind: String,
    pub content: String,
    pub status: String,
    pub source_session: Option<String>,
    pub created_at: i64,
    pub decided_at: Option<i64>,
    /// 적용이 만든 후보 지식의 id. 이 링크가 없으면 무엇을 되무를지 알 수 없다.
    pub applied_memory_id: Option<i64>,
    /// `kind='skill'`일 때 대상 SKILL.md의 절대 경로.
    pub target_path: Option<String>,
    /// 적용 직전 원본. 파일이 없었으면 `None`(신규 생성) — 철회 시 파일을 지운다.
    ///
    /// 스냅샷을 DB에 두는 이유는 스킬이 **git 밖에도 살기** 때문이다. git으로 되돌릴 수
    /// 없으니 되돌릴 것을 우리가 들고 있어야 한다 (DR-4).
    pub snapshot: Option<String>,
    /// 적용 시점의 본문 해시. 철회 때 현재 해시와 다르면 그 뒤로 사람이 손댄 것이므로 거부한다 —
    /// 되돌린다면서 사람의 편집을 조용히 덮는 것이 가장 나쁘다.
    pub applied_hash: Option<String>,
}

/// 신규 DB의 스키마. **`Proposal`의 모든 필드를 여기서 만든다** — 뒤의 `ALTER`에 맡기면
/// 갓 만든 DB조차 "생성 직후 스키마가 한 번 바뀌는" 상태를 거치고, 그 창에서 준비된
/// `SELECT`가 옛 컬럼 목록을 들고 남는다(이슈 #144·#153). `ALTER`는 옛 DB 보강 전용이다.
const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS si_proposals (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  repo           TEXT NOT NULL,
  kind           TEXT NOT NULL,
  content        TEXT NOT NULL,
  status         TEXT NOT NULL DEFAULT 'proposed',
  source_session TEXT,
  created_at     INTEGER NOT NULL,
  decided_at     INTEGER,
  applied_memory_id INTEGER,
  target_path    TEXT,
  snapshot       TEXT,
  applied_hash   TEXT
);
"#;

/// `Proposal`을 `FromRow`로 채우는 데 필요한 컬럼 전부.
///
/// `SELECT *`를 쓰지 않는 이유는 페이로드가 아니라 **컬럼 수를 고정하기 위해서**다. `*`가 무엇으로
/// 펼쳐지는지는 준비 시점의 스키마에 달려 있어, `ALTER TABLE ADD COLUMN`과 겹치면 sqlx가
/// 없는 인덱스를 읽고 worker 스레드가 패닉한다. 이름을 적어 두면 컬럼이 실제로 없을 때
/// prepare 단계에서 즉시 실패한다 — 조용히 어긋난 채 도는 것보다 낫다.
const PROPOSAL_COLUMNS: &str = "id, repo, kind, content, status, source_session, created_at, \
     decided_at, applied_memory_id, target_path, snapshot, applied_hash";

/// 스키마를 만들고, 규격 이전에 만들어진 DB에 빠진 컬럼을 채운다.
///
/// 신규 DB에서는 `MIGRATION`이 이미 전부 만들었으므로 아래 `ALTER`는 모두 no-op이다.
pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    // 커넥션을 고정한다 — `CREATE`와 `ALTER`를 같은 커넥션에서 끝내야 그 커넥션의 스키마 뷰가
    // 도중에 갈라지지 않는다(schedule 모듈도 같은 이유로 이렇게 한다).
    let mut connection = pool.acquire().await?;
    sqlx::query(MIGRATION).execute(&mut *connection).await?;
    // 기존 DB 보강 — 넷 다 nullable이라 이미 쌓인 행은 그대로 유효하다(계획 0038).
    for column in [
        "applied_memory_id INTEGER",
        "target_path TEXT",
        "snapshot TEXT",
        "applied_hash TEXT",
    ] {
        crate::db::add_column_if_missing(&mut *connection, "si_proposals", column).await?;
    }
    Ok(())
}

/// 스킬 본문 제안을 저장한다. 검증에 걸리면 **행을 만들지 않는다** —
/// 적용할 수 없는 제안이 검토 목록에 앉아 있으면 사람의 시간만 쓴다.
pub async fn insert_skill_proposal(
    pool: &SqlitePool,
    repo: &str,
    target_path: &str,
    body: &str,
    source_session: Option<&str>,
    now: i64,
) -> anyhow::Result<i64> {
    validate_skill_body(target_path, body)?;
    let id = sqlx::query(
        "INSERT INTO si_proposals \
         (repo, kind, content, status, source_session, created_at, target_path) \
         VALUES (?, ?, ?, 'proposed', ?, ?, ?)",
    )
    .bind(repo)
    .bind(pkind::SKILL)
    .bind(body)
    .bind(source_session)
    .bind(now)
    .bind(target_path)
    .execute(pool)
    .await?
    .last_insert_rowid();
    Ok(id)
}

pub async fn insert_proposal(
    pool: &SqlitePool,
    repo: &str,
    kind: &str,
    content: &str,
    source_session: Option<&str>,
    now: i64,
) -> anyhow::Result<i64> {
    let id = sqlx::query(
        "INSERT INTO si_proposals (repo, kind, content, status, source_session, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(repo)
    .bind(kind)
    .bind(content)
    .bind(status::PROPOSED)
    .bind(source_session)
    .bind(now)
    .execute(pool)
    .await?
    .last_insert_rowid();
    Ok(id)
}

/// 제안 목록. pending_only=true면 proposed 상태만.
pub async fn list_proposals(
    pool: &SqlitePool,
    pending_only: bool,
) -> anyhow::Result<Vec<Proposal>> {
    let sql = if pending_only {
        format!("SELECT {PROPOSAL_COLUMNS} FROM si_proposals WHERE status = 'proposed' ORDER BY created_at DESC")
    } else {
        // 결정이 끝난 제안은 계속 쌓이기만 한다 — 화면이 쓰는 만큼만 잘라 온다.
        // 미결정을 먼저 정렬하는 것이 핵심이다. created_at만으로 자르면 오래된 미결정 제안이
        // 최근 처리분에 밀려 조용히 사라지고, 검토 화면이 제 기능을 잃는다.
        format!("SELECT {PROPOSAL_COLUMNS} FROM si_proposals ORDER BY (status = 'proposed') DESC, created_at DESC LIMIT 200")
    };
    Ok(sqlx::query_as::<_, Proposal>(&sql).fetch_all(pool).await?)
}

async fn get(pool: &SqlitePool, id: i64) -> anyhow::Result<Option<Proposal>> {
    Ok(
        sqlx::query_as::<_, Proposal>(&format!(
            "SELECT {PROPOSAL_COLUMNS} FROM si_proposals WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(pool)
        .await?,
    )
}

async fn set_status(pool: &SqlitePool, id: i64, st: &str, now: i64) -> anyhow::Result<()> {
    sqlx::query("UPDATE si_proposals SET status = ?, decided_at = ? WHERE id = ?")
        .bind(st)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 적용 — proposed 상태일 때만 후보 지식으로 이동한다. 사람의 evidence 검토 전에는 주입되지 않는다.
/// 반환: 생성된 candidate id (없으면 None).
pub async fn apply_proposal(pool: &SqlitePool, id: i64, now: i64) -> anyhow::Result<Option<i64>> {
    let Some(p) = get(pool, id).await? else {
        anyhow::bail!("제안을 찾을 수 없습니다");
    };
    if p.status != status::PROPOSED {
        anyhow::bail!("이미 처리된 제안입니다");
    }
    let mem_id = memory::create_candidate(
        pool,
        tier::PROJECT,
        Some(&p.repo),
        memory::knowledge_type::OBSERVATION,
        &p.content,
        p.source_session.as_deref(),
        now,
    )
    .await?;
    sqlx::query(
        "UPDATE si_proposals SET status = ?, decided_at = ?, applied_memory_id = ? WHERE id = ?",
    )
    .bind(status::APPLIED)
    .bind(now)
    .bind(mem_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(Some(mem_id))
}

/// 철회 — 적용을 되무른다. 연결된 후보 지식을 archive로 보내고 제안을 `withdrawn`으로 옮긴다.
///
/// 지식을 물리 삭제하지 않는 이유는 `memory::archive`와 같다 — 본문·증거·주입 이력을 보존한다.
/// 링크가 없는 옛 제안(이 필드 도입 전 적용분)은 무엇을 되무를지 알 수 없으므로 거절한다.
pub async fn withdraw_proposal(pool: &SqlitePool, id: i64, now: i64) -> anyhow::Result<()> {
    let Some(p) = get(pool, id).await? else {
        anyhow::bail!("제안을 찾을 수 없습니다");
    };
    if p.status != status::APPLIED {
        anyhow::bail!("적용된 제안만 철회할 수 있습니다");
    }
    let Some(mem_id) = p.applied_memory_id else {
        anyhow::bail!("연결된 지식이 기록되지 않은 제안입니다 — 지식 화면에서 직접 보관하세요");
    };
    // 지식이 이미 승격·보관된 뒤라면 archive가 거절한다. 그 판단은 memory 쪽 전이표에 맡긴다.
    memory::archive(pool, mem_id, now).await?;
    set_status(pool, id, status::WITHDRAWN, now).await
}

/// 거부 — proposed 상태일 때만.
pub async fn reject_proposal(pool: &SqlitePool, id: i64, now: i64) -> anyhow::Result<()> {
    let Some(p) = get(pool, id).await? else {
        anyhow::bail!("제안을 찾을 수 없습니다");
    };
    if p.status != status::PROPOSED {
        anyhow::bail!("이미 처리된 제안입니다");
    }
    set_status(pool, id, status::REJECTED, now).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        memory::migrate(&pool).await.unwrap();
        migrate(&pool).await.unwrap();
        pool
    }

    /// migrate를 지나지 않은 빈 DB. 마이그레이션 자체를 검사할 때 쓴다.
    async fn empty_pool() -> SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    async fn column_count(pool: &SqlitePool) -> i64 {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info('si_proposals')")
            .fetch_one(pool)
            .await
            .unwrap();
        n
    }

    /// 새 DB는 `CREATE TABLE` 한 번으로 완성된다 — 뒤의 `ALTER`가 스키마를 다시 흔들지 않는다.
    ///
    /// 이 불변식이 깨지면 생성 직후 준비된 `SELECT`가 옛 컬럼 목록을 들고 남아, 병렬 실행에서
    /// sqlx worker가 없는 인덱스를 읽고 패닉한다(이슈 #144·#153).
    #[tokio::test]
    async fn a_fresh_database_gets_every_column_from_create_table() {
        let pool = empty_pool().await;
        sqlx::query(MIGRATION).execute(&pool).await.unwrap();
        assert_eq!(column_count(&pool).await, 12);
    }

    /// 규격 이전(8컬럼) DB도 보강 후 그대로 읽힌다 — 더해지는 컬럼은 전부 nullable이다.
    #[tokio::test]
    async fn migrate_backfills_columns_missing_from_an_old_database() {
        let pool = empty_pool().await;
        sqlx::query(
            "CREATE TABLE si_proposals (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               repo TEXT NOT NULL,
               kind TEXT NOT NULL,
               content TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'proposed',
               source_session TEXT,
               created_at INTEGER NOT NULL,
               decided_at INTEGER
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO si_proposals (repo, kind, content, status, created_at) \
             VALUES ('/repo', 'reflection', '보강 전에 쌓인 행', 'proposed', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        migrate(&pool).await.unwrap();

        assert_eq!(column_count(&pool).await, 12);
        let pending = list_proposals(&pool, true).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].target_path.is_none());
    }

    /// 두 번 불러도 실패하지 않는다 — 앱은 켤 때마다 이 함수를 지난다.
    #[tokio::test]
    async fn migrate_is_idempotent() {
        let pool = empty_pool().await;
        migrate(&pool).await.unwrap();
        migrate(&pool).await.unwrap();
        assert_eq!(column_count(&pool).await, 12);
    }

    /// duplicate가 **아닌** `ALTER` 실패는 올라온다. 삼키면 컬럼이 빠진 채 앱이 돌고,
    /// 목록 조회가 런타임에야 깨질 때는 원인이 마이그레이션이라는 단서가 남지 않는다.
    #[tokio::test]
    async fn a_failing_alter_is_not_swallowed() {
        let pool = empty_pool().await; // 테이블이 아직 없다
        let err = crate::db::add_column_if_missing(&pool, "si_proposals", "target_path TEXT")
            .await
            .expect_err("테이블이 없으면 컬럼을 더할 수 없다");
        assert!(err.to_string().contains("target_path"), "{err}");
    }

    #[tokio::test]
    async fn skill_proposal_requires_target_path() {
        let pool = pool().await;
        let r = insert_skill_proposal(&pool, "repo", "", "본문", None, 100).await;
        assert!(r.is_err(), "대상 경로 없는 스킬 제안은 적용할 곳이 없다");
    }

    #[tokio::test]
    async fn skill_proposal_rejects_reserved_markers() {
        let pool = pool().await;
        let body = "앞\n<!-- praxis:capsule begin -->\n뒤";
        let r = insert_skill_proposal(&pool, "repo", "a/SKILL.md", body, None, 100).await;
        assert!(r.is_err(), "예약 마커는 투영 블록 경계를 오인시킨다 (#28)");
    }

    #[tokio::test]
    async fn skill_proposal_rejects_oversized_body() {
        let pool = pool().await;
        let body = "가".repeat(70_000); // 64KB 초과
        let r = insert_skill_proposal(&pool, "repo", "a/SKILL.md", &body, None, 100).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn a_rejected_skill_proposal_leaves_no_row() {
        let pool = pool().await;
        let _ = insert_skill_proposal(&pool, "repo", "", "본문", None, 100).await;
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM si_proposals")
            .fetch_one(&pool)
            .await
            .unwrap();
        // 적용할 수 없는 제안이 검토 목록에 앉아 있으면 사람의 시간만 쓴다.
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn a_valid_skill_proposal_keeps_its_target_and_kind() {
        let pool = pool().await;
        let id = insert_skill_proposal(&pool, "repo", "a/SKILL.md", "본문", Some("s1"), 100)
            .await
            .unwrap();
        let (kind, target): (String, Option<String>) =
            sqlx::query_as("SELECT kind, target_path FROM si_proposals WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(kind, pkind::SKILL);
        assert_eq!(target.as_deref(), Some("a/SKILL.md"));
    }

    async fn proposed(pool: &SqlitePool) -> i64 {
        insert_proposal(pool, "repo", pkind::REFLECTION, "교훈 한 줄", Some("s1"), 1)
            .await
            .unwrap()
    }

    async fn status_of(pool: &SqlitePool, id: i64) -> String {
        get(pool, id).await.unwrap().unwrap().status
    }

    #[tokio::test]
    async fn apply_records_the_memory_it_created() {
        let pool = pool().await;
        let id = proposed(&pool).await;

        let mem_id = apply_proposal(&pool, id, 2).await.unwrap().unwrap();

        let p = get(&pool, id).await.unwrap().unwrap();
        assert_eq!(p.status, status::APPLIED);
        // 링크가 없으면 무엇을 되무를지 알 수 없다 — 철회의 전제.
        assert_eq!(p.applied_memory_id, Some(mem_id));
    }

    #[tokio::test]
    async fn withdraw_archives_the_linked_memory() {
        let pool = pool().await;
        let id = proposed(&pool).await;
        let mem_id = apply_proposal(&pool, id, 2).await.unwrap().unwrap();

        withdraw_proposal(&pool, id, 3).await.unwrap();

        assert_eq!(status_of(&pool, id).await, status::WITHDRAWN);
        let mem_status: String = sqlx::query_scalar("SELECT status FROM memories WHERE id = ?")
            .bind(mem_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mem_status, memory::knowledge_status::ARCHIVED);
    }

    #[tokio::test]
    async fn withdraw_rejects_proposals_that_were_never_applied() {
        let pool = pool().await;
        let id = proposed(&pool).await;

        assert!(withdraw_proposal(&pool, id, 2).await.is_err());
        assert_eq!(status_of(&pool, id).await, status::PROPOSED);
    }

    #[tokio::test]
    async fn withdraw_is_not_repeatable() {
        let pool = pool().await;
        let id = proposed(&pool).await;
        apply_proposal(&pool, id, 2).await.unwrap();
        withdraw_proposal(&pool, id, 3).await.unwrap();

        // 두 번째 철회가 통과하면 이미 보관된 지식을 다시 건드리게 된다.
        assert!(withdraw_proposal(&pool, id, 4).await.is_err());
    }

    #[tokio::test]
    async fn withdraw_refuses_when_the_link_is_missing() {
        let pool = pool().await;
        let id = proposed(&pool).await;
        // 링크 필드 도입 전에 적용된 옛 행을 재현한다.
        sqlx::query("UPDATE si_proposals SET status = ?, applied_memory_id = NULL WHERE id = ?")
            .bind(status::APPLIED)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();

        assert!(withdraw_proposal(&pool, id, 3).await.is_err());
        assert_eq!(status_of(&pool, id).await, status::APPLIED);
    }

    #[tokio::test]
    async fn listing_everything_never_truncates_pending_behind_decided() {
        let pool = pool().await;
        // 오래된 미결정 한 건 — 이후 처리분이 상한을 가득 채운다.
        let old_pending =
            insert_proposal(&pool, "repo", pkind::REFLECTION, "오래된 미결정", None, 1)
                .await
                .unwrap();
        for i in 0..250 {
            let id = insert_proposal(&pool, "repo", pkind::REFLECTION, "처리분", None, 100 + i)
                .await
                .unwrap();
            reject_proposal(&pool, id, 200 + i).await.unwrap();
        }

        let all = list_proposals(&pool, false).await.unwrap();

        assert_eq!(all.len(), 200, "상한이 걸려 있어야 한다");
        assert!(
            all.iter().any(|p| p.id == old_pending),
            "미결정 제안은 상한에 밀려 사라지면 안 된다"
        );
    }

    #[tokio::test]
    async fn migration_adds_the_link_column_to_legacy_tables() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE si_proposals (\
             id INTEGER PRIMARY KEY AUTOINCREMENT, repo TEXT NOT NULL, kind TEXT NOT NULL, \
             content TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'proposed', source_session TEXT, \
             created_at INTEGER NOT NULL, decided_at INTEGER)",
        )
        .execute(&pool)
        .await
        .unwrap();

        migrate(&pool).await.unwrap();

        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('si_proposals') WHERE name = 'applied_memory_id'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 1);
    }
}
