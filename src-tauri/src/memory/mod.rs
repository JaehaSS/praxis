//! 계층형 메모리 스토어 (Phase 3: project tier + FTS5). Tauri 비의존 — `cargo test`로 검증.
//!
//! 같은 SQLite(praxis.sqlite)에 memories 테이블 + FTS5 외부콘텐츠 인덱스.
//! Phase 4에서 global tier + 시맨틱(sqlite-vec)을 추가한다. (shared tier는 설계 0008에서 제거 — 진입점 없이 죽은 개념)

use std::collections::HashMap;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row, SqlitePool};

pub(super) const MARK_START: &str = "<!-- PRAXIS MEMORY START -->";
pub(super) const MARK_END: &str = "<!-- PRAXIS MEMORY END -->";

pub mod application_policy;
pub mod citation;
pub mod confirm_approval;
pub mod context_audit;
pub mod context_summary;
/// 파일형 메모리(P1) — 설계 2026-09-13. 옛 추출 파이프라인을 대신한다.
pub mod file;
mod injection_audit;
pub mod management;
mod projection;
mod projection_plan;
mod projection_recovery;
mod projection_retirement;
mod projection_state;
mod projection_verify;
mod receipt;
mod receipt_finalize;
pub mod restore_error;
pub(crate) mod user_confirmation;
mod version_restore;
pub mod versioning;
#[cfg(test)]
mod knowledge_type_tests;
pub use confirm_approval::{ConfirmApprovalFailure, ConfirmApprovalFailureKind, ConfirmedApproval};
pub use injection_audit::{injections_for_task, InjectedMemory};
pub use projection::{
    inject_into_worktree, verify_task_projection, verify_task_projection_for_start,
    ProjectionStartReceipt,
};
pub use projection_recovery::reconcile_prepared_projections;
pub use projection_retirement::{retire_task_projection, retire_task_projection_if_present};

pub mod tier {
    pub const PROJECT: &str = "project";
    pub const GLOBAL: &str = "global";
}

pub mod kind {
    pub const FACT: &str = "fact";
    pub const DECISION: &str = "decision";
    pub const CONVENTION: &str = "convention";
    pub const REFLECTION: &str = "reflection";
    pub const SKILL: &str = "skill";
}

/// 지식의 의미 유형. `kind`는 기존 행 호환용으로 남기고, 새 흐름은 이 값만 사용한다.
pub mod knowledge_type {
    pub const CLAIM: &str = "claim";
    pub const OBSERVATION: &str = "observation";
    pub const DECISION: &str = "decision";
    pub const CONVENTION: &str = "convention";
    /// 시도했다 접은 접근 — 접은 이유와 재시도 조건을 함께 담는다.
    pub const ABANDONED: &str = "abandoned";
    /// 이어받는 사람이 빠질 함정·비자명한 전제.
    pub const PITFALL: &str = "pitfall";

    pub fn is_valid(value: &str) -> bool {
        matches!(
            value,
            CLAIM | OBSERVATION | DECISION | CONVENTION | ABANDONED | PITFALL
        )
    }
}

/// 지식 검토 상태. 후보/레거시/stale는 어떤 경우에도 컨텍스트 주입 자격이 없다.
pub mod knowledge_status {
    pub const CANDIDATE: &str = "candidate";
    pub const PENDING_REVIEW: &str = "pending_review";
    pub const VERIFIED: &str = "verified";
    pub const STALE: &str = "stale";
    pub const REJECTED: &str = "rejected";
    pub const ARCHIVED: &str = "archived";
    pub const LEGACY_UNVERIFIED: &str = "legacy_unverified";
}

/// 영구 삭제된 메모리의 버전 이력에 남는 표식. 감사 행은 남기되 본문만 이것으로 덮는다.
/// 트리거의 예외 조건도 이 값을 보간해 만들므로, 진실의 원천은 여기 하나다.
pub const PURGED_TOMBSTONE: &str = "[영구 삭제된 메모리]";

pub mod evidence_kind {
    pub const CODE_LOCATION: &str = "code_location";
    pub const TEST_RUN: &str = "test_run";
    pub const DOCUMENT: &str = "document";
    pub const USER_CONFIRMATION: &str = "user_confirmation";

    pub fn is_valid(value: &str) -> bool {
        matches!(
            value,
            CODE_LOCATION | TEST_RUN | DOCUMENT | USER_CONFIRMATION
        )
    }
}

pub mod evidence_status {
    pub const VALID: &str = "valid";
    pub const CHANGED: &str = "changed";
    pub const MISSING: &str = "missing";
    pub const EXPIRED: &str = "expired";
    pub const UNKNOWN: &str = "unknown";
}

/// 주입은 사람 승인을 받은 현재 지식과 유효 증거가 함께 있을 때만 허용한다.
pub fn is_injection_eligible(status: &str, has_valid_evidence: bool) -> bool {
    status == knowledge_status::VERIFIED && has_valid_evidence
}

/// 상태 전이 허용표. 승인으로 가는 길은 항상 pending review를 거친다.
pub fn can_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        (
            knowledge_status::CANDIDATE,
            knowledge_status::PENDING_REVIEW
        ) | (knowledge_status::CANDIDATE, knowledge_status::ARCHIVED)
            | (knowledge_status::PENDING_REVIEW, knowledge_status::VERIFIED)
            | (knowledge_status::PENDING_REVIEW, knowledge_status::REJECTED)
            | (
                knowledge_status::PENDING_REVIEW,
                knowledge_status::CANDIDATE
            )
            | (knowledge_status::VERIFIED, knowledge_status::STALE)
            | (knowledge_status::VERIFIED, knowledge_status::ARCHIVED)
            | (knowledge_status::STALE, knowledge_status::PENDING_REVIEW)
            | (knowledge_status::STALE, knowledge_status::ARCHIVED)
            | (knowledge_status::REJECTED, knowledge_status::ARCHIVED)
            | (
                knowledge_status::LEGACY_UNVERIFIED,
                knowledge_status::PENDING_REVIEW
            )
            | (
                knowledge_status::LEGACY_UNVERIFIED,
                knowledge_status::ARCHIVED
            )
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Memory {
    pub id: i64,
    pub tier: String,
    pub scope_key: Option<String>,
    pub kind: String,
    pub content: String,
    pub source_session: Option<String>,
    pub confidence: f64,
    pub usage_count: i64,
    pub last_used: Option<i64>,
    pub created_at: i64,
    pub knowledge_type: String,
    pub status: String,
    pub current_version: i64,
    pub utility_score: f64,
    pub review_due_at: Option<i64>,
    pub verified_at: Option<i64>,
    pub stale_at: Option<i64>,
    pub archived_at: Option<i64>,
    /// 컨텍스트 주입 방식(`application_policy::policy::*`). 기본은 `relevance` —
    /// 검색 순위를 따른다. `must_apply`만 순위와 무관하게 항상 투영된다.
    pub application_policy: String,
}

const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS memories (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  tier           TEXT NOT NULL,
  scope_key      TEXT,
  kind           TEXT NOT NULL,
  content        TEXT NOT NULL,
  source_session TEXT,
  confidence     REAL NOT NULL DEFAULT 0.5,
  usage_count    INTEGER NOT NULL DEFAULT 0,
  last_used      INTEGER,
  created_at     INTEGER NOT NULL,
  knowledge_type TEXT NOT NULL DEFAULT 'claim',
  status         TEXT NOT NULL DEFAULT 'legacy_unverified',
  current_version INTEGER NOT NULL DEFAULT 0,
  utility_score  REAL NOT NULL DEFAULT 0.5,
  review_due_at  INTEGER,
  verified_at    INTEGER,
  stale_at       INTEGER,
  archived_at    INTEGER,
  application_policy TEXT NOT NULL DEFAULT 'relevance'
);
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts
  USING fts5(content, content='memories', content_rowid='id');
CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
  INSERT INTO memories_fts(rowid, content) VALUES (new.id, new.content);
END;
CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, content) VALUES('delete', old.id, old.content);
END;
CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, content) VALUES('delete', old.id, old.content);
  INSERT INTO memories_fts(rowid, content) VALUES (new.id, new.content);
END;
CREATE TABLE IF NOT EXISTS memory_versions (
  memory_id       INTEGER NOT NULL,
  version         INTEGER NOT NULL,
  content         TEXT NOT NULL,
  knowledge_type  TEXT NOT NULL,
  scope_snapshot  TEXT,
  created_at      INTEGER NOT NULL,
  editor_kind     TEXT NOT NULL,
  PRIMARY KEY (memory_id, version)
);
CREATE TRIGGER IF NOT EXISTS memory_versions_no_delete
BEFORE DELETE ON memory_versions BEGIN
  SELECT RAISE(ABORT, 'memory version cannot be deleted');
END;
CREATE TRIGGER IF NOT EXISTS memory_versions_immutable
BEFORE UPDATE ON memory_versions
WHEN NOT (NEW.content = '[영구 삭제된 메모리]' AND OLD.content <> '[영구 삭제된 메모리]'
          AND NEW.memory_id = OLD.memory_id AND NEW.version = OLD.version
          AND NEW.knowledge_type = OLD.knowledge_type
          AND NEW.scope_snapshot IS OLD.scope_snapshot
          AND NEW.created_at = OLD.created_at
          AND NEW.editor_kind = OLD.editor_kind) BEGIN
  SELECT RAISE(ABORT, 'memory version is immutable');
END;
CREATE TABLE IF NOT EXISTS memory_evidence (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  memory_id       INTEGER NOT NULL,
  version         INTEGER NOT NULL,
  kind            TEXT NOT NULL,
  locator_json    TEXT NOT NULL,
  snapshot_hash   TEXT,
  status          TEXT NOT NULL,
  observed_at     INTEGER NOT NULL,
  checked_at      INTEGER,
  expires_at      INTEGER
);
CREATE TABLE IF NOT EXISTS memory_evidence_checks (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  evidence_id     INTEGER NOT NULL,
  memory_id       INTEGER NOT NULL,
  version         INTEGER NOT NULL,
  status          TEXT NOT NULL,
  observed_hash   TEXT,
  checked_at      INTEGER NOT NULL
);
CREATE TRIGGER IF NOT EXISTS memory_evidence_identity_immutable
BEFORE UPDATE OF memory_id, version, kind, locator_json, snapshot_hash, observed_at, expires_at
ON memory_evidence BEGIN
  SELECT RAISE(ABORT, 'memory evidence identity is immutable');
END;
CREATE TRIGGER IF NOT EXISTS memory_evidence_no_delete
BEFORE DELETE ON memory_evidence BEGIN
  SELECT RAISE(ABORT, 'memory evidence cannot be deleted');
END;
CREATE TRIGGER IF NOT EXISTS memory_evidence_checks_immutable
BEFORE UPDATE ON memory_evidence_checks BEGIN
  SELECT RAISE(ABORT, 'memory evidence checks are immutable');
END;
CREATE TRIGGER IF NOT EXISTS memory_evidence_checks_no_delete
BEFORE DELETE ON memory_evidence_checks BEGIN
  SELECT RAISE(ABORT, 'memory evidence checks cannot be deleted');
END;
CREATE TABLE IF NOT EXISTS memory_approval_receipts (
  id                    INTEGER PRIMARY KEY AUTOINCREMENT,
  memory_id             INTEGER NOT NULL,
  version               INTEGER NOT NULL,
  source_check_ids_json TEXT NOT NULL,
  approved_at           INTEGER NOT NULL
);
CREATE TRIGGER IF NOT EXISTS memory_approval_receipts_immutable
BEFORE UPDATE ON memory_approval_receipts BEGIN
  SELECT RAISE(ABORT, 'memory approval receipt is immutable');
END;
CREATE TRIGGER IF NOT EXISTS memory_approval_receipts_no_delete
BEFORE DELETE ON memory_approval_receipts BEGIN
  SELECT RAISE(ABORT, 'memory approval receipt cannot be deleted');
END;
CREATE TABLE IF NOT EXISTS memory_events (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  memory_id       INTEGER NOT NULL,
  version         INTEGER,
  action          TEXT NOT NULL,
  actor_kind      TEXT NOT NULL,
  reason          TEXT,
  payload_json    TEXT,
  created_at      INTEGER NOT NULL
);
CREATE TRIGGER IF NOT EXISTS memory_events_immutable
BEFORE UPDATE ON memory_events BEGIN
  SELECT RAISE(ABORT, 'memory event is immutable');
END;
CREATE TRIGGER IF NOT EXISTS memory_events_no_delete
BEFORE DELETE ON memory_events BEGIN
  SELECT RAISE(ABORT, 'memory event cannot be deleted');
END;
CREATE TABLE IF NOT EXISTS memory_injections (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  memory_id       INTEGER NOT NULL,
  version         INTEGER NOT NULL,
  task_id         INTEGER NOT NULL,
  target_hash     TEXT NOT NULL,
  injected_at     INTEGER NOT NULL,
  outcome         TEXT,
  projection_id   INTEGER,
  evidence_snapshot_json TEXT NOT NULL DEFAULT '[]',
  source_check_ids_json TEXT NOT NULL DEFAULT '[]',
  target_paths_json TEXT NOT NULL DEFAULT '[]',
  renderer_version INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS memory_projection_journal (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id         INTEGER NOT NULL UNIQUE,
  state           TEXT NOT NULL,
  worktree_path   TEXT NOT NULL,
  target_paths_json TEXT NOT NULL,
  target_hash     TEXT NOT NULL,
  renderer_version INTEGER NOT NULL,
  ordered_memories_json TEXT NOT NULL,
  source_check_ids_json TEXT NOT NULL DEFAULT '[]',
  preimages_json  TEXT,
  failure_reason  TEXT,
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS verification_profiles (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  repo            TEXT NOT NULL,
  label           TEXT NOT NULL,
  argv_json       TEXT NOT NULL,
  cwd_rel         TEXT NOT NULL,
  created_at      INTEGER NOT NULL,
  enabled         INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS verification_runs (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  profile_id      INTEGER NOT NULL,
  source_revision TEXT,
  exit_code       INTEGER,
  stdout_hash     TEXT,
  started_at      INTEGER NOT NULL,
  finished_at     INTEGER
);
CREATE INDEX IF NOT EXISTS memory_evidence_lookup ON memory_evidence(memory_id, version, status);
CREATE INDEX IF NOT EXISTS memory_evidence_checks_lookup ON memory_evidence_checks(evidence_id, checked_at);
CREATE INDEX IF NOT EXISTS memory_events_lookup ON memory_events(memory_id, created_at);
CREATE INDEX IF NOT EXISTS memory_injections_task ON memory_injections(task_id);
"#;

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    // 트리거 본문의 내부 `;` 때문에 수동 분할은 깨진다 → 다중문 실행 API 사용.
    sqlx::raw_sql(MIGRATION).execute(pool).await?;
    // 버전 이력 불변 트리거는 tombstone 전이 하나만 예외로 뚫는다 — 영구 삭제가 본문을 덮을 수
    // 있어야 하되, 덮은 뒤 되돌리거나 다른 컬럼을 고치는 것은 계속 막는다(이슈 #149).
    // 예외 없는 옛 트리거가 남은 DB만 교체한다 — `CREATE TRIGGER IF NOT EXISTS`는 기존 정의를
    // 바꾸지 않기 때문이다. 조건 없이 매번 DROP/CREATE 하면 그때마다 스키마가 바뀌어, 뒤이어
    // 도는 다른 모듈의 `ALTER TABLE ADD COLUMN`이 캐시된 statement와 어긋나 조용히 깨진다.
    let installed: Option<String> = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'memory_versions_immutable'",
    )
    .fetch_optional(pool)
    .await?;
    if installed.is_some_and(|sql| !sql.contains(PURGED_TOMBSTONE)) {
        sqlx::raw_sql(&format!(
            "DROP TRIGGER IF EXISTS memory_versions_immutable; \
             CREATE TRIGGER memory_versions_immutable \
             BEFORE UPDATE ON memory_versions \
             WHEN NOT (NEW.content = '{t}' AND OLD.content <> '{t}' \
                       AND NEW.memory_id = OLD.memory_id AND NEW.version = OLD.version \
                       AND NEW.knowledge_type = OLD.knowledge_type \
                       AND NEW.scope_snapshot IS OLD.scope_snapshot \
                       AND NEW.created_at = OLD.created_at \
                       AND NEW.editor_kind = OLD.editor_kind) \
             BEGIN SELECT RAISE(ABORT, 'memory version is immutable'); END;",
            t = PURGED_TOMBSTONE
        ))
        .execute(pool)
        .await?;
    }
    ensure_memory_columns(pool).await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS memories_knowledge_lookup ON memories(status, tier, scope_key)",
    )
    .execute(pool)
    .await?;
    // Phase 4: 시맨틱 임베딩 컬럼 (기존 DB는 ALTER, 이미 있으면 에러 무시).
    let _ = sqlx::query("ALTER TABLE memories ADD COLUMN embedding BLOB")
        .execute(pool)
        .await;
    // 메모리 주입↔작업 검토 결과 관측.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS memory_usages (\
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           memory_id INTEGER NOT NULL, \
           task_id INTEGER NOT NULL, \
           injected_at INTEGER NOT NULL, \
           outcome TEXT)",
    )
    .execute(pool)
    .await?;
    // 조회 인덱스 — injections_for_task/usages_for_memory/record_review_outcome 풀스캔 방지.
    sqlx::query("CREATE INDEX IF NOT EXISTS mu_task ON memory_usages(task_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS mu_memory ON memory_usages(memory_id)")
        .execute(pool)
        .await?;
    ensure_projection_schema(pool).await?;
    migrate_legacy_memories(pool).await?;
    ensure_file_schema(pool).await?;
    Ok(())
}

/// 파일형 메모리의 **상태만** 담는 표(설계 2026-09-13 R8). 본문은 들어오지 않는다 —
/// 정본은 `<memory_root>` 아래 파일이고 이 행은 크기·시각의 색인일 뿐이다.
async fn ensure_file_schema(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS memory_files (\
           path TEXT PRIMARY KEY, \
           kind TEXT NOT NULL, \
           repo TEXT, \
           repo_key TEXT, \
           lines INTEGER NOT NULL DEFAULT 0, \
           bytes INTEGER NOT NULL DEFAULT 0, \
           modified_at INTEGER, \
           last_projected_at INTEGER, \
           last_task_id INTEGER)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn ensure_projection_schema(pool: &SqlitePool) -> anyhow::Result<()> {
    const INJECTION_COLUMNS: [(&str, &str); 5] = [
        ("projection_id", "INTEGER"),
        ("evidence_snapshot_json", "TEXT NOT NULL DEFAULT '[]'"),
        ("source_check_ids_json", "TEXT NOT NULL DEFAULT '[]'"),
        ("target_paths_json", "TEXT NOT NULL DEFAULT '[]'"),
        ("renderer_version", "INTEGER NOT NULL DEFAULT 1"),
    ];
    ensure_table_columns(pool, "memory_injections", &INJECTION_COLUMNS).await?;
    ensure_table_columns(
        pool,
        "memory_projection_journal",
        &[("source_check_ids_json", "TEXT NOT NULL DEFAULT '[]'")],
    )
    .await?;
    ensure_table_columns(pool, "memory_usages", &[("injection_id", "INTEGER")]).await?;
    sqlx::raw_sql(
        "CREATE UNIQUE INDEX IF NOT EXISTS mi_projection_memory \
           ON memory_injections(projection_id, memory_id, version) WHERE projection_id IS NOT NULL; \
         CREATE UNIQUE INDEX IF NOT EXISTS mu_injection \
           ON memory_usages(injection_id) WHERE injection_id IS NOT NULL; \
         CREATE TRIGGER IF NOT EXISTS memory_injections_identity_immutable \
           BEFORE UPDATE OF memory_id, version, task_id, target_hash, injected_at, projection_id, \
                            evidence_snapshot_json, source_check_ids_json, target_paths_json, renderer_version \
           ON memory_injections WHEN OLD.projection_id IS NOT NULL \
           BEGIN SELECT RAISE(ABORT, 'memory injection identity is immutable'); END; \
         CREATE TRIGGER IF NOT EXISTS memory_injections_no_delete \
           BEFORE DELETE ON memory_injections WHEN OLD.projection_id IS NOT NULL \
           BEGIN SELECT RAISE(ABORT, 'memory injection receipts cannot be deleted'); END; \
         CREATE TRIGGER IF NOT EXISTS memory_projection_identity_immutable \
           BEFORE UPDATE OF task_id, worktree_path, target_paths_json, target_hash, renderer_version, \
                            ordered_memories_json, created_at \
           ON memory_projection_journal \
           BEGIN SELECT RAISE(ABORT, 'memory projection identity is immutable'); END; \
         CREATE TRIGGER IF NOT EXISTS memory_injections_source_checks_immutable \
           BEFORE UPDATE OF source_check_ids_json ON memory_injections \
           WHEN OLD.projection_id IS NOT NULL \
           BEGIN SELECT RAISE(ABORT, 'memory injection source checks are immutable'); END; \
         CREATE TRIGGER IF NOT EXISTS memory_projection_source_checks_final \
           BEFORE UPDATE OF source_check_ids_json ON memory_projection_journal \
           WHEN OLD.state != 'prepared' \
           BEGIN SELECT RAISE(ABORT, 'memory projection source checks are final'); END; \
         CREATE TRIGGER IF NOT EXISTS memory_projection_no_delete \
           BEFORE DELETE ON memory_projection_journal \
           BEGIN SELECT RAISE(ABORT, 'memory projection journals cannot be deleted'); END;",
    )
    .execute(pool)
    .await?;
    // 인용 관측 원장 — 주입 항목의 실사용 증거(설계 0048). 관측이므로 자격·랭킹과 무관하다.
    // marker 판정은 이진이라 cited만 낸다(uncertain은 llm 전용) — CHECK가 강제.
    // 같은 (task, memory, version, method) 재관측은 무시된다 — convo는 턴이 여러 번 끝난다.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS memory_citations ( \
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           task_id INTEGER NOT NULL, \
           memory_id INTEGER NOT NULL, \
           version INTEGER NOT NULL, \
           application_policy TEXT NOT NULL, \
           method TEXT NOT NULL CHECK(method IN ('marker','llm')), \
           verdict TEXT NOT NULL CHECK(verdict IN ('cited','uncertain')), \
           session_id TEXT, \
           created_at INTEGER NOT NULL, \
           CHECK(method = 'llm' OR verdict = 'cited')); \
         CREATE UNIQUE INDEX IF NOT EXISTS mc_dedupe \
           ON memory_citations(task_id, memory_id, version, method); \
         CREATE INDEX IF NOT EXISTS mc_memory ON memory_citations(memory_id); \
         CREATE TRIGGER IF NOT EXISTS memory_citations_immutable \
           BEFORE UPDATE ON memory_citations \
           BEGIN SELECT RAISE(ABORT, 'memory citation is immutable'); END; \
         CREATE TRIGGER IF NOT EXISTS memory_citations_no_delete \
           BEFORE DELETE ON memory_citations \
           BEGIN SELECT RAISE(ABORT, 'memory citation cannot be deleted'); END;",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn ensure_table_columns(
    pool: &SqlitePool,
    table: &str,
    columns: &[(&str, &str)],
) -> anyhow::Result<()> {
    for (name, definition) in columns {
        let query = format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?");
        let exists: Option<(i64,)> = sqlx::query_as(&query)
            .bind(name)
            .fetch_optional(pool)
            .await?;
        if exists.is_none() {
            let sql = format!("ALTER TABLE {table} ADD COLUMN {name} {definition}");
            sqlx::query(&sql).execute(pool).await?;
        }
    }
    Ok(())
}

async fn ensure_memory_columns(pool: &SqlitePool) -> anyhow::Result<()> {
    const COLUMNS: [(&str, &str); 9] = [
        ("knowledge_type", "TEXT NOT NULL DEFAULT 'claim'"),
        ("status", "TEXT NOT NULL DEFAULT 'legacy_unverified'"),
        ("current_version", "INTEGER NOT NULL DEFAULT 0"),
        ("utility_score", "REAL NOT NULL DEFAULT 0.5"),
        ("review_due_at", "INTEGER"),
        ("verified_at", "INTEGER"),
        ("stale_at", "INTEGER"),
        ("archived_at", "INTEGER"),
        // 기존 행은 전부 relevance로 읽힌다 — 이 migration은 값을 바꾸지도,
        // 감사 event를 만들지도 않는다.
        ("application_policy", "TEXT NOT NULL DEFAULT 'relevance'"),
    ];
    for (name, definition) in COLUMNS {
        let exists: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM pragma_table_info('memories') WHERE name = ?")
                .bind(name)
                .fetch_optional(pool)
                .await?;
        if exists.is_none() {
            let sql = format!("ALTER TABLE memories ADD COLUMN {name} {definition}");
            sqlx::query(&sql).execute(pool).await?;
        }
    }
    Ok(())
}

fn legacy_knowledge_type(kind: &str) -> &'static str {
    match kind {
        kind::DECISION => knowledge_type::DECISION,
        kind::CONVENTION => knowledge_type::CONVENTION,
        _ => knowledge_type::CLAIM,
    }
}

/// 이전 kind 또는 UI 입력을 현재 지식 유형으로 보수적으로 정규화한다.
pub fn normalize_knowledge_type(value: &str) -> &'static str {
    // LLM 추출 kind의 공백·대소문자 변주를 여기서 흡수한다 — 호출처가 각자 정규화하지 않게.
    match value.trim().to_lowercase().as_str() {
        knowledge_type::OBSERVATION => knowledge_type::OBSERVATION,
        knowledge_type::DECISION => knowledge_type::DECISION,
        knowledge_type::CONVENTION => knowledge_type::CONVENTION,
        knowledge_type::ABANDONED => knowledge_type::ABANDONED,
        knowledge_type::PITFALL => knowledge_type::PITFALL,
        _ => knowledge_type::CLAIM,
    }
}

pub fn validate_knowledge_content(content: &str) -> anyhow::Result<()> {
    if content.trim().is_empty() {
        anyhow::bail!("지식 본문은 비어 있을 수 없습니다");
    }
    if content.contains(MARK_START) || content.contains(MARK_END) {
        anyhow::bail!("지식 본문에는 Praxis 예약 마커를 넣을 수 없습니다");
    }
    Ok(())
}

async fn migrate_legacy_memories(pool: &SqlitePool) -> anyhow::Result<()> {
    let (legacy_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM memories WHERE current_version = 0")
            .fetch_one(pool)
            .await?;
    if legacy_count == 0 {
        return Ok(());
    }
    // 외부 콘텐츠 FTS5를 구형 행이 있는 DB에 처음 만들면 인덱스가 비어 있다. 이후 UPDATE 트리거가
    // 없는 FTS 행을 삭제하려 하지 않도록, 이관 전에 현재 content table로 인덱스를 재구성한다.
    sqlx::query("INSERT INTO memories_fts(memories_fts) VALUES('rebuild')")
        .execute(pool)
        .await
        .context("legacy memory FTS 재구성")?;
    let mut tx = pool.begin().await?;
    let rows = sqlx::query(
        "SELECT id, kind, content, scope_key, created_at FROM memories WHERE current_version = 0",
    )
    .fetch_all(&mut *tx)
    .await
    .context("legacy memory rows 읽기")?;
    for row in rows {
        let id: i64 = row.try_get("id")?;
        let kind: String = row.try_get("kind")?;
        let content: String = row.try_get("content")?;
        let scope_key: Option<String> = row.try_get("scope_key")?;
        let created_at: i64 = row.try_get("created_at")?;
        let knowledge_type = legacy_knowledge_type(&kind);

        sqlx::query(
            "UPDATE memories SET knowledge_type = ?, status = ?, current_version = 1, utility_score = confidence WHERE id = ?",
        )
        .bind(knowledge_type)
        .bind(knowledge_status::LEGACY_UNVERIFIED)
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("legacy memory 상태 이관")?;
        sqlx::query(
            "INSERT INTO memory_versions (memory_id, version, content, knowledge_type, scope_snapshot, created_at, editor_kind) \
             VALUES (?, 1, ?, ?, ?, ?, 'system_migration')",
        )
        .bind(id)
        .bind(content)
        .bind(knowledge_type)
        .bind(scope_key)
        .bind(created_at)
        .execute(&mut *tx)
        .await
        .context("legacy memory version 기록")?;
        sqlx::query(
            "INSERT INTO memory_events (memory_id, version, action, actor_kind, created_at) \
             VALUES (?, 1, 'legacy_migrated', 'system', ?)",
        )
        .bind(id)
        .bind(created_at)
        .execute(&mut *tx)
        .await
        .context("legacy memory 감사 이벤트 기록")?;
    }
    tx.commit().await.context("legacy memory 이관 커밋")?;
    Ok(())
}

/// 작업 생성 시 worktree에 주입하는 메모리 상한 — inject_into_worktree와 memory_preview가 공유(발산 방지).
pub const INJECTION_LIMIT: i64 = 8;

/// 휴면: 생성 DORMANT_DAYS일 경과 && 한 번도 주입 안 됨 → 후보 제외(삭제 아님 — 가역, 설계 0008 규칙).
pub const DORMANT_DAYS: i64 = 30;

/// verified 상태이면서 현재 본문 version의 모든 evidence가 유효한 지식만 후보로 삼는다.
/// SQL 시각은 DB 자체의 `strftime('%s','now')`를 써서 별도 now 인자 없이 조회 시점 기준으로 판정한다.
fn active_filter(table: &str) -> String {
    format!(
        "{table}.status = '{verified}' \
         AND EXISTS (SELECT 1 FROM memory_evidence e \
                     WHERE e.memory_id = {table}.id \
                       AND e.version = {table}.current_version \
                       AND e.status = '{evidence_valid}') \
         AND NOT EXISTS (SELECT 1 FROM memory_evidence e \
                         WHERE e.memory_id = {table}.id \
                           AND e.version = {table}.current_version \
                           AND (e.status != '{evidence_valid}' \
                                OR e.expires_at <= CAST(strftime('%s','now') AS INTEGER))) \
         AND NOT ({table}.usage_count = 0 AND {table}.created_at < CAST(strftime('%s','now') AS INTEGER) - {dormant_days} * 86400)",
        verified = knowledge_status::VERIFIED,
        evidence_valid = evidence_status::VALID,
        dormant_days = DORMANT_DAYS,
    )
}

/// 휴면 판정 — `active_filter()`의 시간 기준과 동일(usage_count=0 && DORMANT_DAYS 경과).
/// SQL 필터와 별개로 Rust 측(목록 API의 dormant 배지 등, T2)에서 재사용할 수 있게 노출.
pub fn is_dormant(m: &Memory, now: i64) -> bool {
    m.usage_count == 0 && now - m.created_at > DORMANT_DAYS * 86400
}

// ── 시맨틱 검색 (Phase 4): 임베딩은 외부(embed 모듈)에서 주입, 여기선 저장/코사인만 ──

pub(crate) fn encode_f32(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub(crate) fn decode_f32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// 코사인 유사도 (-1..1). 길이 0이면 0.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let (mut na, mut nb) = (0.0f32, 0.0f32);
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

pub async fn set_embedding(pool: &SqlitePool, id: i64, emb: &[f32]) -> anyhow::Result<()> {
    sqlx::query("UPDATE memories SET embedding = ? WHERE id = ?")
        .bind(encode_f32(emb))
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 임베딩 제거(NULL) — 내용 수정 후 재임베딩 실패 시 낡은 벡터로 잘못 랭킹되지 않도록 FTS-only로 강등.
pub async fn clear_embedding(pool: &SqlitePool, id: i64) -> anyhow::Result<()> {
    sqlx::query("UPDATE memories SET embedding = NULL WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(super) async fn set_embedding_for_version(
    pool: &SqlitePool,
    id: i64,
    version: i64,
    emb: &[f32],
) -> anyhow::Result<()> {
    sqlx::query("UPDATE memories SET embedding = ? WHERE id = ? AND current_version = ?")
        .bind(encode_f32(emb))
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
    Ok(())
}

pub(super) async fn clear_embedding_for_version(
    pool: &SqlitePool,
    id: i64,
    version: i64,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE memories SET embedding = NULL WHERE id = ? AND current_version = ?")
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
    Ok(())
}

/// 시맨틱 검색 — 계층 스코프 내 임베딩 보유 메모리를 코사인 유사도로 랭킹.
pub async fn semantic_search(
    pool: &SqlitePool,
    repo: &str,
    query_emb: &[f32],
    limit: i64,
) -> anyhow::Result<Vec<Memory>> {
    let sql = format!(
        "SELECT * FROM memories \
         WHERE embedding IS NOT NULL \
           AND ((tier = 'project' AND scope_key = ?) OR tier = 'global') AND {}",
        active_filter("memories")
    );
    let rows = sqlx::query(&sql).bind(repo).fetch_all(pool).await?;
    let mut scored: Vec<(f32, Memory)> = Vec::with_capacity(rows.len());
    for r in &rows {
        let blob: Vec<u8> = r.try_get("embedding")?;
        let score = cosine(query_emb, &decode_f32(&blob));
        scored.push((score, Memory::from_row(r)?));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored
        .into_iter()
        .take(limit.max(0) as usize)
        .map(|(_, m)| m)
        .collect())
}

/// scope(repo) 내 project tier 기존 메모리 임베딩과의 최대 코사인 유사도 — 삽입 시 dedupe 판정용
/// (capture 모듈 재사용, `cosine`/`decode_f32` 로직 중복 구현 금지). 후보 없으면 0.0.
pub async fn max_project_cosine(pool: &SqlitePool, repo: &str, emb: &[f32]) -> anyhow::Result<f32> {
    let rows = sqlx::query(
        "SELECT embedding FROM memories WHERE embedding IS NOT NULL AND tier = ? AND scope_key = ?",
    )
    .bind(tier::PROJECT)
    .bind(repo)
    .fetch_all(pool)
    .await?;
    let mut best = 0.0f32;
    for r in &rows {
        let blob: Vec<u8> = r.try_get("embedding")?;
        let score = cosine(emb, &decode_f32(&blob));
        if score > best {
            best = score;
        }
    }
    Ok(best)
}

/// 하이브리드 검색 — FTS(retrieve_layered) + 시맨틱을 RRF로 융합. query_emb 없으면 FTS만.
pub async fn retrieve_hybrid(
    pool: &SqlitePool,
    repo: &str,
    query_text: &str,
    query_emb: Option<&[f32]>,
    limit: i64,
) -> anyhow::Result<Vec<Memory>> {
    let fts = retrieve_layered(pool, repo, query_text, limit * 2).await?;
    let sem = match query_emb {
        Some(e) => semantic_search(pool, repo, e, limit * 2).await?,
        None => Vec::new(),
    };
    if sem.is_empty() {
        return Ok(fts.into_iter().take(limit.max(0) as usize).collect());
    }
    const K: f32 = 60.0;
    let mut score: HashMap<i64, f32> = HashMap::new();
    let mut byid: HashMap<i64, Memory> = HashMap::new();
    for (rank, m) in fts.iter().enumerate() {
        *score.entry(m.id).or_insert(0.0) += 1.0 / (K + rank as f32 + 1.0);
        byid.entry(m.id).or_insert_with(|| m.clone());
    }
    for (rank, m) in sem.iter().enumerate() {
        *score.entry(m.id).or_insert(0.0) += 1.0 / (K + rank as f32 + 1.0);
        byid.entry(m.id).or_insert_with(|| m.clone());
    }
    let mut merged: Vec<(f32, Memory)> = score
        .into_iter()
        .filter_map(|(id, s)| byid.remove(&id).map(|m| (s, m)))
        .collect();
    merged.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(merged
        .into_iter()
        .take(limit.max(0) as usize)
        .map(|(_, m)| m)
        .collect())
}

/// 임의 텍스트를 안전한 FTS5 MATCH 쿼리로 변환 (영숫자 토큰만, OR 결합).
pub fn fts_query(text: &str) -> String {
    let tokens: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(|t| t.to_lowercase())
        .collect();
    tokens
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub async fn insert(
    pool: &SqlitePool,
    tier: &str,
    scope_key: Option<&str>,
    kind: &str,
    content: &str,
    source_session: Option<&str>,
    now: i64,
) -> anyhow::Result<i64> {
    let id = sqlx::query(
        "INSERT INTO memories (tier, scope_key, kind, content, source_session, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(tier)
    .bind(scope_key)
    .bind(kind)
    .bind(content)
    .bind(source_session)
    .bind(now)
    .execute(pool)
    .await?
    .last_insert_rowid();
    Ok(id)
}

/// 자동 추출·수동 입력·제안 공용 후보 생성.
/// 세션은 provenance일 뿐 승인 증거가 아니며, 후보는 유효 증거와 사람 검토 전에는 주입될 수 없다.
pub async fn create_candidate(
    pool: &SqlitePool,
    tier: &str,
    scope_key: Option<&str>,
    knowledge_type: &str,
    content: &str,
    source_session: Option<&str>,
    now: i64,
) -> anyhow::Result<i64> {
    if !knowledge_type::is_valid(knowledge_type) {
        anyhow::bail!("지원하지 않는 지식 유형");
    }
    validate_knowledge_content(content)?;
    let mut tx = pool.begin().await?;
    let id = sqlx::query(
        "INSERT INTO memories (tier, scope_key, kind, content, source_session, created_at, knowledge_type, status, current_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(tier)
    .bind(scope_key)
    .bind(knowledge_type)
    .bind(content)
    .bind(source_session)
    .bind(now)
    .bind(knowledge_type)
    .bind(knowledge_status::CANDIDATE)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    sqlx::query(
        "INSERT INTO memory_versions (memory_id, version, content, knowledge_type, scope_snapshot, created_at, editor_kind) \
         VALUES (?, 1, ?, ?, ?, ?, 'candidate_intake')",
    )
    .bind(id)
    .bind(content)
    .bind(knowledge_type)
    .bind(scope_key)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO memory_events (memory_id, version, action, actor_kind, created_at) \
         VALUES (?, 1, 'candidate_created', 'system', ?)",
    )
    .bind(id)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Tauri user action에서만 만드는 확인 receipt. 클라이언트 locator를 신뢰 경계 안으로 들이지 않는다.
pub async fn add_user_confirmation(
    pool: &SqlitePool,
    memory_id: i64,
    observed_at: i64,
    expires_at: Option<i64>,
) -> anyhow::Result<i64> {
    let mut tx = pool.begin().await?;
    let version: Option<i64> =
        sqlx::query_scalar("SELECT current_version FROM memories WHERE id = ?")
            .bind(memory_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(version) = version else {
        anyhow::bail!("메모리를 찾을 수 없습니다");
    };
    let evidence =
        user_confirmation::insert(&mut tx, memory_id, version, observed_at, expires_at).await?;
    tx.commit().await?;
    Ok(evidence.id)
}

/// 현재 버전에 evidence가 있고 모두 valid·미만료인지 확인한다.
pub async fn has_valid_evidence(
    pool: &SqlitePool,
    memory_id: i64,
    version: i64,
    now: i64,
) -> anyhow::Result<bool> {
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM( \
           CASE WHEN status = ? AND (expires_at IS NULL OR expires_at > ?) THEN 1 ELSE 0 END \
         ), 0) FROM memory_evidence WHERE memory_id = ? AND version = ?",
    )
    .bind(evidence_status::VALID)
    .bind(now)
    .bind(memory_id)
    .bind(version)
    .fetch_one(pool)
    .await?;
    Ok(counts.0 > 0 && counts.0 == counts.1)
}

/// 후보 또는 stale 지식을 사람 검토 큐로 보낸다.
pub async fn submit_for_review(pool: &SqlitePool, memory_id: i64, now: i64) -> anyhow::Result<()> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT status, current_version FROM memories WHERE id = ?")
            .bind(memory_id)
            .fetch_optional(pool)
            .await?;
    let Some((status, version)) = row else {
        anyhow::bail!("메모리를 찾을 수 없습니다");
    };
    if !can_transition(&status, knowledge_status::PENDING_REVIEW) {
        anyhow::bail!("현재 상태에서는 검토를 요청할 수 없습니다");
    }
    sqlx::query("UPDATE memories SET status = ? WHERE id = ? AND status = ?")
        .bind(knowledge_status::PENDING_REVIEW)
        .bind(memory_id)
        .bind(&status)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO memory_events (memory_id, version, action, actor_kind, created_at) \
         VALUES (?, ?, 'review_submitted', 'human', ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// 사람만 현재 version의 유효 증거가 달린 검토 대기 항목을 승인할 수 있다.
pub async fn approve(
    pool: &SqlitePool,
    memory_id: i64,
    actor_kind: &str,
    now: i64,
) -> anyhow::Result<()> {
    if actor_kind != "human" {
        anyhow::bail!("사람만 지식을 승인할 수 있음");
    }
    crate::evidence::approve_memory(pool, memory_id, now).await
}

/// 사람 확인·검토 제출·현재 근거 검증·승인 receipt를 한 transaction으로 확정한다.
pub async fn confirm_and_approve(
    pool: &SqlitePool,
    memory_id: i64,
    expected_version: i64,
    now: i64,
) -> confirm_approval::ConfirmApprovalResult<confirm_approval::ConfirmedApproval> {
    crate::evidence::confirm_and_approve_memory(pool, memory_id, expected_version, now).await
}

/// 본문/유형 수정은 새 version을 만들고 기존 승인과 증거를 재사용하지 못하게 candidate로 되돌린다.
pub async fn update_knowledge(
    pool: &SqlitePool,
    memory_id: i64,
    content: &str,
    knowledge_type: &str,
    now: i64,
) -> anyhow::Result<()> {
    if !knowledge_type::is_valid(knowledge_type) {
        anyhow::bail!("지원하지 않는 지식 유형");
    }
    validate_knowledge_content(content)?;
    let mut tx = pool.begin().await?;
    let row: Option<(i64, Option<String>, String)> = sqlx::query_as(
        "SELECT current_version, scope_key, application_policy FROM memories WHERE id = ?",
    )
    .bind(memory_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((current_version, scope_key, previous_policy)) = row else {
        anyhow::bail!("메모리를 찾을 수 없습니다");
    };
    let next_version = current_version + 1;
    sqlx::query(
        "UPDATE memories SET content = ?, kind = ?, knowledge_type = ?, status = ?, current_version = ?, \
                            verified_at = NULL, stale_at = NULL, archived_at = NULL WHERE id = ?",
    )
    .bind(content)
    .bind(knowledge_type)
    .bind(knowledge_type)
    .bind(knowledge_status::CANDIDATE)
    .bind(next_version)
    .bind(memory_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO memory_versions (memory_id, version, content, knowledge_type, scope_snapshot, created_at, editor_kind) \
         VALUES (?, ?, ?, ?, ?, ?, 'human_edit')",
    )
    .bind(memory_id)
    .bind(next_version)
    .bind(content)
    .bind(knowledge_type)
    .bind(scope_key)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO memory_events (memory_id, version, action, actor_kind, created_at) \
         VALUES (?, ?, 'content_updated', 'human', ?)",
    )
    .bind(memory_id)
    .bind(next_version)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    // 승인은 그 version의 본문에 대한 것이다 — 본문이 바뀌면 지정도 따라오지 않는다.
    application_policy::reset_on_lifecycle_change(
        &mut tx,
        memory_id,
        &previous_policy,
        next_version,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// 물리 삭제 대신 archive로 전환해 본문·증거·주입 이력을 보존한다.
pub async fn archive(pool: &SqlitePool, memory_id: i64, now: i64) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    let row: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT status, current_version, application_policy FROM memories WHERE id = ?",
    )
    .bind(memory_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((status, version, previous_policy)) = row else {
        anyhow::bail!("메모리를 찾을 수 없습니다");
    };
    if status == knowledge_status::ARCHIVED || !can_transition(&status, knowledge_status::ARCHIVED)
    {
        anyhow::bail!("현재 상태에서는 보관할 수 없습니다");
    }
    let updated =
        sqlx::query("UPDATE memories SET status = ?, archived_at = ? WHERE id = ? AND status = ?")
            .bind(knowledge_status::ARCHIVED)
            .bind(now)
            .bind(memory_id)
            .bind(&status)
            .execute(&mut *tx)
            .await?;
    if updated.rows_affected() != 1 {
        anyhow::bail!("메모리 상태가 변경되어 보관하지 못했습니다");
    }
    sqlx::query(
        "INSERT INTO memory_events (memory_id, version, action, actor_kind, created_at) \
         VALUES (?, ?, 'archived', 'human', ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    // 보관된 메모리는 주입 자격 자체가 없다 — 지정을 남기면 복원 시 조용히 되살아난다.
    application_policy::reset_on_lifecycle_change(
        &mut tx,
        memory_id,
        &previous_policy,
        version,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// 프로젝트 메모리 FTS 검색 (관련도 순). 빈 쿼리면 최신순.
pub async fn retrieve_project(
    pool: &SqlitePool,
    scope_key: &str,
    query_text: &str,
    limit: i64,
) -> anyhow::Result<Vec<Memory>> {
    let fq = fts_query(query_text);
    if fq.is_empty() {
        let rows = sqlx::query_as::<_, Memory>(
            "SELECT * FROM memories WHERE tier = ? AND scope_key = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(tier::PROJECT)
        .bind(scope_key)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        return Ok(rows);
    }
    let rows = sqlx::query_as::<_, Memory>(
        "SELECT m.* FROM memories m \
         JOIN memories_fts f ON f.rowid = m.id \
         WHERE memories_fts MATCH ? AND m.tier = ? AND m.scope_key = ? \
         ORDER BY bm25(memories_fts) LIMIT ?",
    )
    .bind(&fq)
    .bind(tier::PROJECT)
    .bind(scope_key)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// `Memory`를 `FromRow`로 채우는 데 필요한 컬럼 전부 — embedding BLOB(≈1.5KB/행)만 뺀다.
///
/// `SELECT *` 대신 이 상수를 쓰는 이유는 페이로드 축소지만, 목록을 손으로 나열하면
/// `Memory`에 필드를 더할 때 여기를 빠뜨려 런타임에 깨진다. 한 곳으로 모아 그 함정을 없앤다.
pub(crate) const MEMORY_COLUMNS: &str = "id, tier, scope_key, kind, content, source_session, \
     confidence, usage_count, last_used, created_at, knowledge_type, status, current_version, \
     utility_score, review_due_at, verified_at, stale_at, archived_at, application_policy";

/// 모든 메모리(최신순) — MemoryView 표시용.
pub async fn list_all(pool: &SqlitePool) -> anyhow::Result<Vec<Memory>> {
    let rows = sqlx::query_as::<_, Memory>(&format!(
        "SELECT {MEMORY_COLUMNS} FROM memories ORDER BY created_at DESC"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 계층 통합 검색 — project(scope=repo) + global(scope NULL)을 FTS 관련도로 병합.
/// 작업 시작 시 컨텍스트 주입에 사용. 빈 쿼리면 최신순.
pub async fn retrieve_layered(
    pool: &SqlitePool,
    repo: &str,
    query_text: &str,
    limit: i64,
) -> anyhow::Result<Vec<Memory>> {
    let fq = fts_query(query_text);
    if fq.is_empty() {
        let sql = format!(
            "SELECT * FROM memories \
             WHERE ((tier = 'project' AND scope_key = ?) OR (tier = 'global')) AND {} \
             ORDER BY created_at DESC LIMIT ?",
            active_filter("memories")
        );
        let rows = sqlx::query_as::<_, Memory>(&sql)
            .bind(repo)
            .bind(limit)
            .fetch_all(pool)
            .await?;
        return Ok(rows);
    }
    let sql = format!(
        "SELECT m.* FROM memories m \
         JOIN memories_fts f ON f.rowid = m.id \
         WHERE memories_fts MATCH ? \
           AND ((m.tier = 'project' AND m.scope_key = ?) OR (m.tier = 'global')) AND {} \
         ORDER BY bm25(memories_fts) LIMIT ?",
        active_filter("m")
    );
    let rows = sqlx::query_as::<_, Memory>(&sql)
        .bind(&fq)
        .bind(repo)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn list_project(pool: &SqlitePool, scope_key: &str) -> anyhow::Result<Vec<Memory>> {
    let rows = sqlx::query_as::<_, Memory>(
        "SELECT * FROM memories WHERE tier = ? AND scope_key = ? ORDER BY created_at DESC",
    )
    .bind(tier::PROJECT)
    .bind(scope_key)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 보관된 메모리를 영구 삭제한다. 본문은 지우고 감사 행은 남긴다.
///
/// `archive`가 상태만 바꾸는 것과 달리 여기서는 `memories` 행 자체가 사라진다 — 본문·embedding이
/// 소실되고 `memories_ad` 트리거가 FTS 인덱스에서도 뺀다. 다만 `memory_versions`는 `no_delete`
/// 계약이 걸린 감사 테이블이라 행을 지우지 않고 본문만 tombstone으로 덮는다. 누가 언제 무엇을
/// 지웠는지는 남고, 지워진 내용만 사라진다.
///
/// `archived`를 거치지 않은 메모리는 대상이 아니다 — 되돌릴 수 없는 작업 앞에 확인 단계를 둔다.
pub async fn purge(pool: &SqlitePool, memory_id: i64, now: i64) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT status, current_version FROM memories WHERE id = ?")
            .bind(memory_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((status, version)) = row else {
        anyhow::bail!("메모리를 찾을 수 없습니다");
    };
    if status != knowledge_status::ARCHIVED {
        anyhow::bail!("보관된 메모리만 영구 삭제할 수 있습니다");
    }
    // 이벤트를 먼저 남긴다 — 행이 사라진 뒤에는 무엇을 지웠는지 기록할 수 없다.
    sqlx::query(
        "INSERT INTO memory_events (memory_id, version, action, actor_kind, created_at) \
         VALUES (?, ?, 'purged', 'human', ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE memory_versions SET content = ? WHERE memory_id = ? AND content <> ?")
        .bind(PURGED_TOMBSTONE)
        .bind(memory_id)
        .bind(PURGED_TOMBSTONE)
        .execute(&mut *tx)
        .await?;
    let deleted = sqlx::query("DELETE FROM memories WHERE id = ? AND status = ?")
        .bind(memory_id)
        .bind(knowledge_status::ARCHIVED)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() != 1 {
        anyhow::bail!("메모리 상태가 변경되어 영구 삭제하지 못했습니다");
    }
    tx.commit().await?;
    Ok(())
}

// ── 사용↔검토 결과 관측 ──

pub mod outcome {
    pub const APPROVED: &str = "approved";
    pub const DISCARDED: &str = "discarded";
}

/// 작업에 주입된 메모리 1건 기록 + usage_count 증가.
pub async fn record_injection(
    pool: &SqlitePool,
    memory_id: i64,
    task_id: i64,
    now: i64,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO memory_usages (memory_id, task_id, injected_at) VALUES (?, ?, ?)")
        .bind(memory_id)
        .bind(task_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE memories SET usage_count = usage_count + 1, last_used = ? WHERE id = ?")
        .bind(now)
        .bind(memory_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// 작업의 검토 결과를 주입 이력에 기록한다.
///
/// 이 결과는 작업 전체의 승인·폐기일 뿐 개별 메모리 품질의 인과 증거가 아니므로 confidence를
/// 변경하거나 검색 후보에서 제외하는 데 사용하지 않는다.
pub async fn record_review_outcome(
    pool: &SqlitePool,
    task_id: i64,
    outcome: &str,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE memory_usages SET outcome = ? WHERE task_id = ? AND outcome IS NULL")
        .bind(outcome)
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE memory_injections SET outcome = ? WHERE task_id = ? AND outcome IS NULL")
        .bind(outcome)
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

// ── 주입 검증/이력/편집 (메모리가 실제로 세션에 반영됐는지 확인) ──

/// 컨텍스트 파일 텍스트에서 주입된 메모리 블록(마커 사이 내용)을 추출. 마커 없으면 None.
/// 에이전트가 읽는 CLAUDE.md에 블록이 실재하는지 확인하는 근거.
pub fn extract_injected_block(text: &str) -> Option<String> {
    // END 앞의 **마지막** START 사용 — 부분쓰기/중복 마커로 START가 여러 개여도 실제 블록만 잡는다.
    let e = text.find(MARK_END)?;
    let region = &text[..e];
    let s = region.rfind(MARK_START)?;
    let block = region[s + MARK_START.len()..].trim();
    if block.is_empty() {
        None // 빈 블록은 "주입됨"으로 보고하지 않는다(false-positive 방지).
    } else {
        Some(block.to_string())
    }
}

/// worktree의 대상 파일들에서 주입 블록을 스캔 → (블록 있는 파일명, 첫 블록 텍스트).
/// 도메인 로직(inject_into_worktree의 읽기측 짝, cargo test 가능). 파일당 크기 상한으로 거대/특수 파일 방어.
pub fn scan_injected_targets(
    worktree_path: &std::path::Path,
    targets: &[&str],
) -> (Vec<String>, Option<String>) {
    const MAX_BYTES: u64 = 4 * 1024 * 1024;
    let mut present = Vec::new();
    let mut block_text: Option<String> = None;
    for f in targets {
        let p = worktree_path.join(f);
        if std::fs::metadata(&p)
            .map(|m| m.len() > MAX_BYTES)
            .unwrap_or(true)
        {
            continue; // 없음/과대/특수 파일 스킵
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        if let Some(block) = extract_injected_block(&text) {
            present.push((*f).to_string());
            if block_text.is_none() {
                block_text = Some(block);
            }
        }
    }
    (present, block_text)
}

/// 특정 메모리가 주입된 작업 한 건 (사용 이력).
#[derive(Debug, Clone, Serialize)]
pub struct MemoryUsageRow {
    pub task_id: i64,
    pub instruction: String,
    pub state: String,
    pub injected_at: i64,
    pub outcome: Option<String>,
}

/// 특정 메모리가 주입된 작업 이력 (memory_usages JOIN tasks) — 최신순.
pub async fn usages_for_memory(
    pool: &SqlitePool,
    memory_id: i64,
) -> anyhow::Result<Vec<MemoryUsageRow>> {
    let rows = sqlx::query(
        "SELECT u.task_id, u.injected_at, u.outcome, t.instruction, t.state \
         FROM memory_usages u JOIN tasks t ON t.id = u.task_id \
         WHERE u.memory_id = ? ORDER BY u.injected_at DESC",
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        out.push(MemoryUsageRow {
            task_id: r.try_get("task_id")?,
            instruction: r.try_get("instruction")?,
            state: r.try_get("state")?,
            injected_at: r.try_get("injected_at")?,
            outcome: r.try_get("outcome")?,
        });
    }
    Ok(out)
}

/// 메모리 내용/종류 수정 (FTS 재색인은 memories_au 트리거가 처리).
pub async fn update_content(
    pool: &SqlitePool,
    id: i64,
    content: &str,
    kind: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE memories SET content = ?, kind = ? WHERE id = ?")
        .bind(content)
        .bind(kind)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
