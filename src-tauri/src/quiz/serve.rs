//! 출제와 채점 (설계 0044).
//!
//! 출제 쿼리는 `status='approved'`만 본다 — 검수 전(`pending`)과 신고된 것(`retired`)은
//! 여기서 구조적으로 빠진다.

use serde::Serialize;
use sqlx::SqlitePool;

use super::generate::Kind;

/// 출제 쿼리 한 행 — (id, kind, question, choices, source_excerpt).
type ItemRow = (i64, String, String, Option<String>, Option<String>);

/// 검수 목록 한 행 — (id, question, choices, answer, explanation, excerpt, doc_title, heading).
type PendingRow = (
    i64,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// 출제된 문제. **정답을 담지 않는다** — 화면 소스에 답이 있으면 스스로를 속이기 쉽다.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QuizItem {
    pub id: i64,
    pub kind: String,
    pub question: String,
    /// 단답형이면 None.
    pub choices: Option<Vec<String>>,
    /// 도메인 문제의 근거 본문. 푸는 동안 함께 보인다 — 검수와 학습을 겸한다.
    pub source_excerpt: Option<String>,
}

/// 채점 결과. 틀렸을 때 정답을 함께 준다 — 모르고 넘어가면 학습이 되지 않는다.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AnswerResult {
    pub correct: bool,
    pub answer: String,
    pub explanation: Option<String>,
}

/// 검수 대기 중인 도메인 문제 — 근거와 함께 본다.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PendingItem {
    pub id: i64,
    pub question: String,
    pub choices: Option<Vec<String>>,
    pub answer: String,
    pub explanation: Option<String>,
    pub source_excerpt: Option<String>,
    pub doc_title: Option<String>,
    pub heading: Option<String>,
}

/// 큐에 무엇이 남았는가 — 패널을 띄울지 정하는 데만 쓴다.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Availability {
    /// 지금 낼 수 있는 문제 수. 풀다 만 문제도 아직 답하지 않았으므로 여기 포함된다.
    pub askable: i64,
    /// 검수 대기 중인 도메인 문제 수.
    pub pending_review: i64,
}

/// 다음 문제를 준다.
///
/// **풀다 만 문제가 있으면 그것부터** — 대기가 끊겨도 다음 대기에서 이어 풀 수 있어야 한다
/// (설계 0044 DR-3). 없으면 승인된 문제 중 하나를 열고 시도를 기록한다.
///
/// `kinds`가 비면 전부를 대상으로 한다.
pub async fn next(
    pool: &SqlitePool,
    kinds: &[String],
    now: i64,
) -> anyhow::Result<Option<QuizItem>> {
    if let Some(open) = fetch_open(pool).await? {
        return Ok(Some(open));
    }

    let Some(filter) = kind_filter(kinds) else {
        return Ok(None);
    };

    // 이미 답한 문제는 다시 내지 않는다. 복습(간격 반복)은 범위 밖이다 — 넣으려면 마지막
    // 정답 시각과 재출제 간격이 필요한데, 그건 이 기능이 쓸 만한지 확인한 뒤의 일이다.
    let row: Option<ItemRow> = sqlx::query_as(&format!(
        "SELECT id, kind, question, choices, source_excerpt FROM quiz_items \
         WHERE status = 'approved'{filter} \
           AND id NOT IN (SELECT item_id FROM quiz_attempts WHERE state = 'answered') \
         ORDER BY RANDOM() LIMIT 1"
    ))
    .fetch_optional(pool)
    .await?;

    let Some((id, kind, question, choices, source_excerpt)) = row else {
        return Ok(None);
    };
    sqlx::query("INSERT INTO quiz_attempts (item_id, state, opened_at) VALUES (?, 'open', ?)")
        .bind(id)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(Some(QuizItem {
        id,
        kind,
        question,
        choices: parse_choices(choices.as_deref()),
        source_excerpt,
    }))
}

/// 채점하고 시도를 닫는다. 이미 닫힌 문제를 다시 채점해도 결과는 같다(멱등).
pub async fn answer(
    pool: &SqlitePool,
    item_id: i64,
    picked: &str,
    now: i64,
) -> anyhow::Result<Option<AnswerResult>> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT answer, explanation FROM quiz_items WHERE id = ?")
            .bind(item_id)
            .fetch_optional(pool)
            .await?;
    let Some((answer, explanation)) = row else {
        return Ok(None);
    };
    let correct = picked.trim() == answer.trim();

    sqlx::query(
        "UPDATE quiz_attempts SET state = 'answered', picked = ?, correct = ?, answered_at = ? \
         WHERE item_id = ? AND state = 'open'",
    )
    .bind(picked)
    .bind(correct as i64)
    .bind(now)
    .bind(item_id)
    .execute(pool)
    .await?;

    Ok(Some(AnswerResult {
        correct,
        answer,
        explanation,
    }))
}

/// 신고 — 다시 출제되지 않는다 (설계 0044 비즈니스 규칙 5).
///
/// 지우지 않고 상태만 바꾸는 이유는 같은 문제가 재생성될 때 중복 검사에 걸리게 하기 위함이다.
pub async fn report(pool: &SqlitePool, item_id: i64, now: i64) -> anyhow::Result<()> {
    sqlx::query("UPDATE quiz_items SET status = 'retired', reviewed_at = ? WHERE id = ?")
        .bind(now)
        .bind(item_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 검수 통과 — 이 시점부터 출제 대상이 된다.
pub async fn approve(pool: &SqlitePool, item_id: i64, now: i64) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE quiz_items SET status = 'approved', reviewed_at = ? WHERE id = ? AND status = 'pending'",
    )
    .bind(now)
    .bind(item_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 검수 대기 목록. 근거를 함께 실어 문제와 나란히 볼 수 있게 한다.
pub async fn pending(pool: &SqlitePool, limit: i64) -> anyhow::Result<Vec<PendingItem>> {
    let rows: Vec<PendingRow> = sqlx::query_as(
        "SELECT i.id, i.question, i.choices, i.answer, i.explanation, i.source_excerpt, \
                c.doc_title, c.heading \
         FROM quiz_items i LEFT JOIN knowledge_chunks c ON c.id = i.chunk_id \
         WHERE i.status = 'pending' ORDER BY i.created_at LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, question, choices, answer, explanation, source_excerpt, doc_title, heading)| {
                PendingItem {
                    id,
                    question,
                    choices: parse_choices(choices.as_deref()),
                    answer,
                    explanation,
                    source_excerpt,
                    doc_title,
                    heading,
                }
            },
        )
        .collect())
}

/// 종류 필터 SQL 조각. 요청한 종류가 **전부** 모르는 것이면 `None`이다.
///
/// 여기서 끊지 않으면 필터가 빈 문자열로 떨어져 "아무것도 요청 안 함"과 같아지고, 요청하지
/// 않은 종류가 새어 나온다.
fn kind_filter(kinds: &[String]) -> Option<String> {
    // `Kind::parse`를 통과한 정적 문자열만 조립한다 — 사용자 입력이 SQL에 직접 닿지 않는다.
    let allowed: Vec<&'static str> = kinds
        .iter()
        .filter_map(|k| Kind::parse(k))
        .map(Kind::as_str)
        .collect();
    if !kinds.is_empty() && allowed.is_empty() {
        return None;
    }
    if allowed.is_empty() {
        return Some(String::new());
    }
    Some(format!(
        " AND kind IN ({})",
        allowed
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(",")
    ))
}

/// 지금 낼 수 있는 문제와 검수 대기가 각각 몇 건인가.
///
/// **문제를 소비하지 않는다.** `next`는 고른 문제의 시도를 열어 기록하므로, 띄울지 말지를
/// 정하는 폴링이 그것을 부르면 아무도 보지 않은 문제가 계속 "풀던 문제"로 쌓인다.
///
/// `kinds`가 비면 전부를 센다 — 화면이 쓰는 경로다. 필터를 준 경우, `next`가 종류와 무관하게
/// 이어 주는 열린 시도(DR-3)는 그 종류가 아니면 여기 안 잡힌다.
pub async fn availability(pool: &SqlitePool, kinds: &[String]) -> anyhow::Result<Availability> {
    // 출제가 0이어도 검수가 남았으면 대기 시간에 할 일이 있다 — 그 판단이 이 값에 달렸다.
    let pending_review: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM quiz_items WHERE status = 'pending'")
            .fetch_one(pool)
            .await?;

    let Some(filter) = kind_filter(kinds) else {
        return Ok(Availability {
            askable: 0,
            pending_review,
        });
    };
    // `next`의 출제 조건과 같아야 한다 — 어긋나면 "있다"고 열어 놓고 빈 화면을 보여준다.
    let askable: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM quiz_items \
         WHERE status = 'approved'{filter} \
           AND id NOT IN (SELECT item_id FROM quiz_attempts WHERE state = 'answered')"
    ))
    .fetch_one(pool)
    .await?;

    Ok(Availability {
        askable,
        pending_review,
    })
}

/// 열려 있는 시도의 문제. 신고·폐기된 문제의 시도는 되살리지 않는다.
async fn fetch_open(pool: &SqlitePool) -> anyhow::Result<Option<QuizItem>> {
    let row: Option<ItemRow> = sqlx::query_as(
        "SELECT i.id, i.kind, i.question, i.choices, i.source_excerpt \
         FROM quiz_attempts a JOIN quiz_items i ON i.id = a.item_id \
         WHERE a.state = 'open' AND i.status = 'approved' \
         ORDER BY a.opened_at LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(
        row.map(|(id, kind, question, choices, source_excerpt)| QuizItem {
            id,
            kind,
            question,
            choices: parse_choices(choices.as_deref()),
            source_excerpt,
        }),
    )
}

/// 저장된 JSON 배열을 되돌린다. 깨져 있으면 단답형으로 취급한다 — 출제를 막지는 않는다.
fn parse_choices(raw: Option<&str>) -> Option<Vec<String>> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .filter(|list| !list.is_empty())
}
