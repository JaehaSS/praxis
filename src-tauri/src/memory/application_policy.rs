//! 메모리가 작업 컨텍스트에 들어가는 **방식**.
//!
//! `relevance`는 기존 하이브리드 검색 순위를 따른다 — 상위 `INJECTION_LIMIT`건에 들지
//! 못하면 주입되지 않는다. `must_apply`는 순위와 무관하게 항상 투영된다.
//!
//! 별도 우선순위 테이블을 두지 않는다. 정책은 메모리 1건당 1값이라 조인만 늘고,
//! 변경 이력은 이미 불변 트리거로 보호되는 `memory_events`가 받는다.

use super::Memory;
use sqlx::{Sqlite, SqlitePool, Transaction};

/// 정책 값. DB에는 문자열로 저장한다(기존 `tier`/`status`와 같은 표현).
pub mod policy {
    /// 기본값. 검색 순위에 따라 선택된다.
    pub const RELEVANCE: &str = "relevance";
    /// 사람이 지정한 항상-적용 규칙. 검색과 분리해 조회한다.
    pub const MUST_APPLY: &str = "must_apply";

    pub fn is_valid(value: &str) -> bool {
        matches!(value, RELEVANCE | MUST_APPLY)
    }
}

/// project별 지정 개수 상한. 초과는 mutation 없이 거부한다 —
/// 조용히 자르면 "항상 적용"이 항상이 아니게 된다.
pub const MAX_MUST_APPLY_PER_PROJECT: usize = 8;
/// project별 지정 본문 합계 상한(UTF-8 바이트).
pub const MAX_MUST_APPLY_BYTES: usize = 8 * 1024;

/// 정책 변경 감사 event의 action.
pub const EVENT_ACTION: &str = "application_policy_changed";

/// 정책 변경 실패 사유. 호출측(Tauri command / Runner HTTP)이 상태 코드로 옮긴다.
#[derive(Debug, PartialEq, Eq)]
pub enum PolicyFailure {
    NotFound,
    /// 알 수 없는 정책 값.
    InvalidPolicy,
    /// 지정 자격 미달 — tier·knowledge_type·status 중 하나가 조건을 벗어난다.
    Ineligible(&'static str),
    /// 개수 또는 바이트 상한 초과.
    CapacityExceeded(String),
    /// 정규화 후 같은 본문이 이미 지정돼 있다.
    Duplicate,
    /// version 또는 policy 기대값 불일치 — 다른 곳에서 먼저 바뀌었다.
    Conflict,
    /// 저장소 오류. 원문(경로·SQL)은 로그에만 남기고 사용자에게는 노출하지 않는다.
    Storage,
}

impl std::fmt::Display for PolicyFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "메모리를 찾을 수 없습니다"),
            Self::InvalidPolicy => write!(f, "지원하지 않는 적용 정책입니다"),
            Self::Ineligible(reason) => write!(f, "{reason}"),
            Self::CapacityExceeded(detail) => write!(f, "{detail}"),
            Self::Duplicate => write!(f, "같은 내용의 규칙이 이미 지정돼 있습니다"),
            Self::Conflict => write!(f, "메모리가 변경되어 정책을 적용하지 못했습니다"),
            Self::Storage => write!(f, "정책을 저장하지 못했습니다"),
        }
    }
}

type PolicyResult<T> = Result<T, PolicyFailure>;

/// 정책 변경 대상 행의 스냅샷.
#[derive(sqlx::FromRow)]
struct PolicyRow {
    tier: String,
    scope_key: Option<String>,
    content: String,
    knowledge_type: String,
    status: String,
    current_version: i64,
    application_policy: String,
}

/// 본문 비교용 정규화 — 공백 차이만 있는 규칙을 같은 것으로 본다.
/// 의미 충돌 탐지는 하지 않는다(LLM 판정 없이 결정론 유지).
fn normalize(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 사람이 지정할 수 있는 대상인가.
///
/// `kind`가 아니라 `knowledge_type`으로 판정한다 — `kind`는 기존 행 호환용이라
/// `kind='decision'`이면서 `knowledge_type='claim'`인 레거시 행이 통과해 버린다.
fn ensure_eligible(row: &PolicyRow) -> PolicyResult<()> {
    if row.tier != super::tier::PROJECT {
        return Err(PolicyFailure::Ineligible(
            "프로젝트 범위 메모리만 항상 적용으로 지정할 수 있습니다",
        ));
    }
    if !matches!(
        row.knowledge_type.as_str(),
        super::knowledge_type::DECISION | super::knowledge_type::CONVENTION
    ) {
        return Err(PolicyFailure::Ineligible(
            "결정·규약 유형만 항상 적용으로 지정할 수 있습니다",
        ));
    }
    if row.status != super::knowledge_status::VERIFIED {
        return Err(PolicyFailure::Ineligible(
            "검증된 메모리만 항상 적용으로 지정할 수 있습니다",
        ));
    }
    Ok(())
}

/// 같은 scope에 이미 지정된 규칙들 — 상한·중복 검사의 기준.
async fn designated_siblings(
    tx: &mut Transaction<'_, Sqlite>,
    memory_id: i64,
    tier: &str,
    scope_key: Option<&str>,
) -> PolicyResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT content FROM memories \
         WHERE application_policy = ? AND tier = ? AND scope_key IS ? AND id != ?",
    )
    .bind(policy::MUST_APPLY)
    .bind(tier)
    .bind(scope_key)
    .bind(memory_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(|(content,)| content).collect())
}

fn ensure_capacity(siblings: &[String], content: &str) -> PolicyResult<()> {
    if siblings.len() + 1 > MAX_MUST_APPLY_PER_PROJECT {
        return Err(PolicyFailure::CapacityExceeded(format!(
            "프로젝트당 항상 적용 규칙은 {MAX_MUST_APPLY_PER_PROJECT}건까지입니다"
        )));
    }
    let total: usize = siblings.iter().map(|s| s.len()).sum::<usize>() + content.len();
    if total > MAX_MUST_APPLY_BYTES {
        return Err(PolicyFailure::CapacityExceeded(format!(
            "항상 적용 규칙 합계는 {MAX_MUST_APPLY_BYTES}바이트까지입니다 (요청 {total}바이트)"
        )));
    }
    Ok(())
}

fn ensure_not_duplicate(siblings: &[String], content: &str) -> PolicyResult<()> {
    let target = normalize(content);
    if siblings.iter().any(|s| normalize(s) == target) {
        return Err(PolicyFailure::Duplicate);
    }
    Ok(())
}

/// 사람의 개별 동작으로만 호출된다 — 자동·일괄 승격 경로는 두지 않는다.
///
/// `expected_version`/`expected_policy`로 CAS한다. 반환값 `false`는 "이미 그 상태였다"로,
/// 응답을 잃은 클라이언트의 재시도가 중복 event를 만들지 않게 한다.
pub async fn set_policy(
    pool: &SqlitePool,
    memory_id: i64,
    next: &str,
    expected_version: i64,
    expected_policy: &str,
    now: i64,
) -> PolicyResult<bool> {
    if !policy::is_valid(next) {
        return Err(PolicyFailure::InvalidPolicy);
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let row: Option<PolicyRow> = sqlx::query_as(
        "SELECT tier, scope_key, content, knowledge_type, status, current_version, application_policy \
         FROM memories WHERE id = ?",
    )
    .bind(memory_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(PolicyFailure::NotFound);
    };

    // version은 항상 엄격하게 본다 — 본문이 바뀌었으면 승인 대상이 달라진 것이다.
    if row.current_version != expected_version {
        return Err(PolicyFailure::Conflict);
    }
    // 목표 상태에 이미 도달했으면 성공으로 흡수한다(lost-response 재시도).
    // `expected_policy` 비교보다 먼저 봐야 재시도가 Conflict로 떨어지지 않는다.
    if row.application_policy == next {
        return Ok(false);
    }
    if row.application_policy != expected_policy {
        return Err(PolicyFailure::Conflict);
    }

    // 지정 해제는 어떤 상태에서도 열어 둔다 — stale 규칙이 작업을 막을 때
    // 사용자가 빠져나갈 길이 없으면 fail-closed가 덫이 된다.
    if next == policy::MUST_APPLY {
        ensure_eligible(&row)?;
        let siblings =
            designated_siblings(&mut tx, memory_id, &row.tier, row.scope_key.as_deref()).await?;
        ensure_not_duplicate(&siblings, &row.content)?;
        ensure_capacity(&siblings, &row.content)?;
    }

    let updated = sqlx::query(
        "UPDATE memories SET application_policy = ? \
         WHERE id = ? AND current_version = ? AND application_policy = ?",
    )
    .bind(next)
    .bind(memory_id)
    .bind(expected_version)
    .bind(expected_policy)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(PolicyFailure::Conflict);
    }
    append_event(&mut tx, memory_id, row.current_version, next, now).await?;
    tx.commit().await?;
    Ok(true)
}

async fn append_event(
    tx: &mut Transaction<'_, Sqlite>,
    memory_id: i64,
    version: i64,
    next: &str,
    now: i64,
) -> PolicyResult<()> {
    sqlx::query(
        "INSERT INTO memory_events (memory_id, version, action, actor_kind, payload_json, created_at) \
         VALUES (?, ?, ?, 'human', ?, ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(EVENT_ACTION)
    .bind(format!(r#"{{"application_policy":"{next}"}}"#))
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 본문·version이 바뀌는 경로에서 정책을 기본값으로 되돌린다.
///
/// 승인은 **그 version의 본문**에 대한 것이다. 편집·복원·보관으로 대상이 달라지면
/// 지정도 따라오지 않는다. 호출측의 기존 transaction 안에서 실행해 부분 적용을 막는다.
/// 실제로 지정돼 있던 경우에만 event를 남긴다(공통 경로의 event 폭주 방지).
/// `sqlx::Result`를 그대로 돌려준다 — anyhow 경로(`update_knowledge`/`archive`)와
/// `RestoreFailure` 경로(version restore) 양쪽이 `?` 한 번으로 받게 하기 위해서다.
pub(super) async fn reset_on_lifecycle_change(
    tx: &mut Transaction<'_, Sqlite>,
    memory_id: i64,
    previous_policy: &str,
    version: i64,
    now: i64,
) -> sqlx::Result<()> {
    if previous_policy != policy::MUST_APPLY {
        return Ok(());
    }
    sqlx::query("UPDATE memories SET application_policy = ? WHERE id = ?")
        .bind(policy::RELEVANCE)
        .bind(memory_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO memory_events (memory_id, version, action, actor_kind, payload_json, created_at) \
         VALUES (?, ?, ?, 'human', ?, ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(EVENT_ACTION)
    .bind(r#"{"application_policy":"relevance","reason":"lifecycle_change"}"#)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 한 작업에 투영할 메모리 — 순서가 계약이다.
#[derive(Debug)]
pub struct Selection {
    /// 항상 적용 규칙. `id ASC`로 고정해 같은 입력이 늘 같은 블록을 만든다(receipt 해시 안정성).
    pub must_apply: Vec<Memory>,
    /// 하이브리드 검색 결과에서 `must_apply`와 겹친 항목을 뺀 나머지.
    pub relevant: Vec<Memory>,
}

impl Selection {
    /// 투영 순서대로 이어붙인 목록 — receipt와 렌더러가 이 순서를 그대로 쓴다.
    pub fn ordered(&self) -> Vec<Memory> {
        let mut all = self.must_apply.clone();
        all.extend(self.relevant.iter().cloned());
        all
    }
}

/// 지정된 규칙을 `id ASC`로 읽는다. 상태는 거르지 않는다 —
/// 자격을 잃은 규칙은 조용히 빠지는 대신 `ensure_all_usable`이 시끄럽게 막는다.
async fn list_must_apply(pool: &SqlitePool, repo: &str) -> anyhow::Result<Vec<Memory>> {
    let rows = sqlx::query_as::<_, Memory>(&format!(
        "SELECT {} FROM memories \
         WHERE application_policy = ? AND tier = ? AND scope_key = ? \
         ORDER BY id ASC",
        super::MEMORY_COLUMNS
    ))
    .bind(policy::MUST_APPLY)
    .bind(super::tier::PROJECT)
    .bind(repo)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 지정 규칙이 지금도 주입 자격을 갖는지. 하나라도 못 갖추면 **작업을 막는다**.
///
/// 조용히 일반 검색으로 강등하면 "항상 적용"이라고 표시해 놓고 실제로는 빠지는 상태가
/// 만들어진다 — 이 기능이 없애려는 문제를 다른 층에서 재현하는 셈이다.
async fn ensure_all_usable(pool: &SqlitePool, rules: &[Memory], now: i64) -> anyhow::Result<()> {
    for rule in rules {
        if rule.status != super::knowledge_status::VERIFIED {
            anyhow::bail!(
                "항상 적용 규칙 #{}이 더 이상 검증 상태가 아닙니다 ({}). \
                 규칙을 다시 검토하거나 지정을 해제한 뒤 시작하세요.",
                rule.id,
                rule.status
            );
        }
        if !super::has_valid_evidence(pool, rule.id, rule.current_version, now).await? {
            anyhow::bail!(
                "항상 적용 규칙 #{}의 근거가 만료되었습니다. \
                 근거를 갱신하거나 지정을 해제한 뒤 시작하세요.",
                rule.id
            );
        }
    }
    Ok(())
}

/// 상한을 넘긴 지정이 이미 저장돼 있으면 자르지 않고 막는다 —
/// 조용히 자르면 어느 규칙이 빠졌는지 아무도 모른다.
fn ensure_within_capacity(rules: &[Memory]) -> anyhow::Result<()> {
    if rules.len() > MAX_MUST_APPLY_PER_PROJECT {
        anyhow::bail!(
            "항상 적용 규칙이 상한({MAX_MUST_APPLY_PER_PROJECT}건)을 넘었습니다 ({}건). \
             일부 지정을 해제한 뒤 시작하세요.",
            rules.len()
        );
    }
    let total: usize = rules.iter().map(|rule| rule.content.len()).sum();
    if total > MAX_MUST_APPLY_BYTES {
        anyhow::bail!(
            "항상 적용 규칙 합계가 상한({MAX_MUST_APPLY_BYTES}바이트)을 넘었습니다 ({total}바이트). \
             일부 지정을 해제한 뒤 시작하세요."
        );
    }
    Ok(())
}

/// 작업에 투영할 메모리를 결정론적으로 고른다.
///
/// **preview와 실제 projection이 같은 함수를 쓴다** — 갈라지면 UI가 보여준 것과
/// 에이전트가 받은 것이 달라진다.
///
/// 관련 메모리는 `limit`만큼 채운다. 지정 규칙과 겹쳐 빠진 자리는 확대 조회로 되채운다 —
/// 그러지 않으면 규칙을 지정할수록 일반 컨텍스트가 조용히 줄어든다.
pub async fn select_for_projection(
    pool: &SqlitePool,
    repo: &str,
    instruction: &str,
    query_embedding: Option<&[f32]>,
    limit: i64,
    now: i64,
) -> anyhow::Result<Selection> {
    let must_apply = list_must_apply(pool, repo).await?;
    ensure_within_capacity(&must_apply)?;
    ensure_all_usable(pool, &must_apply, now).await?;

    let widened = super::retrieve_hybrid(
        pool,
        repo,
        instruction,
        query_embedding,
        limit + must_apply.len() as i64,
    )
    .await?;
    let designated: std::collections::HashSet<i64> = must_apply.iter().map(|m| m.id).collect();
    let relevant = widened
        .into_iter()
        .filter(|memory| !designated.contains(&memory.id))
        .take(limit as usize)
        .collect();
    Ok(Selection {
        must_apply,
        relevant,
    })
}

impl From<sqlx::Error> for PolicyFailure {
    fn from(error: sqlx::Error) -> Self {
        // 진단은 로그로, 사용자에게는 원인 종류만. 저장소 오류를 Conflict로 뭉개면
        // 디스크·잠금 문제가 "누가 먼저 바꿨다"로 오보고된다.
        eprintln!("application_policy 저장소 오류: {error}");
        Self::Storage
    }
}
