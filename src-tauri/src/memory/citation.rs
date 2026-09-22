//! 메모리 인용 관측 — 주입 항목이 세션에서 실제 사용됐는지 판정해 append-only 원장에 기록.
//! 관측일 뿐이다 — confidence·자격·랭킹을 바꾸지 않는다 (ADR 0038 · 설계 0048 DR-2).
//!
//! 판정 소스는 applied journal이 동결한 receipt다 — `memory_injections`의 legacy 행은
//! version이 없어 원장 행을 만들 수 없다 (플랜 0047 DR-P1).

use sqlx::SqlitePool;

use super::receipt::MemoryReceipt;

pub mod method {
    pub const MARKER: &str = "marker";
    pub const LLM: &str = "llm";
}

pub mod verdict {
    pub const CITED: &str = "cited";
    pub const UNCERTAIN: &str = "uncertain";
}

/// LLM 합승 판정에 필요한 재료 — fragment는 캡처 프롬프트에 그대로 끼운다.
pub struct RideAlong {
    pub fragment: String,
    pub(super) receipts: Vec<MemoryReceipt>,
}

/// ASCII 토큰 `M-{id}`의 경계 안전 검색 — 앞은 영숫자/'-' 금지, 뒤는 숫자 금지.
/// (needle이 ASCII라 경계의 byte 검사가 안전하다 — 멀티바이트 이웃은 조건을 통과한다)
fn transcript_cites(text: &str, memory_id: i64) -> bool {
    let needle = format!("M-{memory_id}");
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(pos) = text[from..].find(&needle) {
        let start = from + pos;
        let end = start + needle.len();
        let prev_ok = start == 0 || {
            let c = bytes[start - 1];
            !c.is_ascii_alphanumeric() && c != b'-'
        };
        let next_ok = end >= bytes.len() || !bytes[end].is_ascii_digit();
        if prev_ok && next_ok {
            return true;
        }
        from = end;
    }
    false
}

/// applied journal이 동결한 receipt — 없으면 빈 벡터(주입 없던 작업).
async fn applied_receipts(pool: &SqlitePool, task_id: i64) -> anyhow::Result<Vec<MemoryReceipt>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT ordered_memories_json FROM memory_projection_journal \
         WHERE task_id = ? AND state = 'applied'",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?;
    match row {
        Some((json,)) => Ok(serde_json::from_str(&json)?),
        None => Ok(Vec::new()),
    }
}

/// 관측 1행 — 같은 (task, memory, version, method) 재관측은 무시된다(멱등).
async fn record(
    pool: &SqlitePool,
    task_id: i64,
    session_id: Option<&str>,
    receipt: &MemoryReceipt,
    method: &str,
    verdict: &str,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO memory_citations \
         (task_id, memory_id, version, application_policy, method, verdict, session_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(task_id)
    .bind(receipt.memory_id)
    .bind(receipt.version)
    .bind(&receipt.application_policy)
    .bind(method)
    .bind(verdict)
    .bind(session_id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// 캡처 프롬프트에 합승할 판정 지시. content는 앞 200자만(프롬프트 비대 방지).
fn llm_prompt_fragment(receipts: &[MemoryReceipt]) -> String {
    let mut s = String::from(
        "\n추가 임무: 아래 '주입 메모리' 각각이 <TRANSCRIPT> 세션에서 실제로 활용됐는지 판정하라. \
         활용 = 내용을 참조·적용했음(주입 블록이 보였다는 것만으로는 활용이 아니다). \
         확실히 활용 → cited, 애매 → uncertain, 활용 안 됨 → 제외. \
         출력 마지막 줄에 정확히 이 형식 한 줄만 추가: \
         PRAXIS_CITATIONS: {\"cited\":[id…],\"uncertain\":[id…]}\n주입 메모리:\n",
    );
    for receipt in receipts {
        let head: String = receipt.content.chars().take(200).collect();
        s.push_str(&format!("- {}: {}\n", receipt.memory_id, head));
    }
    s
}

/// 캡처 stdout에서 판정 섹션(`PRAXIS_CITATIONS:` 줄)을 분리한다 — (정화본, 섹션 JSON).
/// 캡처의 배열 파싱은 "첫 `[` ~ 마지막 `]`" 폴백이라, 이 줄을 먼저 걷어내지 않으면
/// 판정 JSON까지 삼켜 캡처가 조용히 0건이 된다 (플랜 0047 DR-P4).
pub fn split_section(stdout: &str) -> (String, Option<String>) {
    const PREFIX: &str = "PRAXIS_CITATIONS:";
    let mut cleaned = String::with_capacity(stdout.len());
    let mut section = None;
    for line in stdout.lines() {
        match line.trim_start().strip_prefix(PREFIX) {
            Some(rest) => section = Some(rest.trim().to_string()),
            None => {
                cleaned.push_str(line);
                cleaned.push('\n');
            }
        }
    }
    (cleaned, section)
}

#[derive(serde::Deserialize)]
struct LlmSection {
    #[serde(default)]
    cited: Vec<i64>,
    #[serde(default)]
    uncertain: Vec<i64>,
}

/// LLM 판정 섹션을 파싱해 기록한다. id는 ride의 receipt 화이트리스트로 제한 —
/// 트랜스크립트는 신뢰할 수 없는 데이터라 모델이 없는 id를 뱉을 수 있다.
pub async fn record_llm(
    pool: &SqlitePool,
    task_id: i64,
    session_id: Option<&str>,
    ride: &RideAlong,
    section: &str,
    now: i64,
) -> anyhow::Result<usize> {
    let parsed: LlmSection = serde_json::from_str(section)?;
    let mut n = 0;
    for (ids, verdict) in [
        (&parsed.cited, verdict::CITED),
        (&parsed.uncertain, verdict::UNCERTAIN),
    ] {
        for id in ids {
            if let Some(receipt) = ride.receipts.iter().find(|r| r.memory_id == *id) {
                record(pool, task_id, session_id, receipt, method::LLM, verdict, now).await?;
                n += 1;
            }
        }
    }
    Ok(n)
}

/// 턴 종료 관측 진입점. 주입 없던 작업이면 None(0비용). marker 판정은 즉시 기록하고
/// LLM 합승 재료를 돌려준다. best-effort는 호출부(`let _ =`) 책임이다.
pub async fn observe(
    pool: &SqlitePool,
    task_id: i64,
    session_id: Option<&str>,
    transcript: &str,
    now: i64,
) -> anyhow::Result<Option<RideAlong>> {
    let receipts = applied_receipts(pool, task_id).await?;
    if receipts.is_empty() {
        return Ok(None);
    }
    for receipt in &receipts {
        if transcript_cites(transcript, receipt.memory_id) {
            record(
                pool, task_id, session_id, receipt, method::MARKER, verdict::CITED, now,
            )
            .await?;
        }
    }
    Ok(Some(RideAlong {
        fragment: llm_prompt_fragment(&receipts),
        receipts,
    }))
}

/// Context Inspector용 read-only 분리 집계 — 규칙형(must_apply)과 사실형(relevance)을
/// 가른다. 규칙형은 잘 작동할수록 인용 흔적이 없으므로(설계 0048 DR-4) 합산하면 오독된다.
#[derive(Debug, serde::Serialize)]
pub struct CitationCounts {
    pub must_apply_injected: i64,
    pub must_apply_cited: i64,
    pub relevance_injected: i64,
    pub relevance_cited: i64,
}

/// 작업 단위 인용 집계. 주입 없던 작업은 None — 0/0 표시는 "관측했는데 없음"과 구별돼야 한다.
pub async fn summary(pool: &SqlitePool, task_id: i64) -> anyhow::Result<Option<CitationCounts>> {
    let receipts = applied_receipts(pool, task_id).await?;
    if receipts.is_empty() {
        return Ok(None);
    }
    let must_apply = super::application_policy::policy::MUST_APPLY;
    let mut counts = CitationCounts {
        must_apply_injected: 0,
        must_apply_cited: 0,
        relevance_injected: 0,
        relevance_cited: 0,
    };
    for receipt in &receipts {
        if receipt.application_policy == must_apply {
            counts.must_apply_injected += 1;
        } else {
            counts.relevance_injected += 1;
        }
    }
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT application_policy, COUNT(DISTINCT memory_id) FROM memory_citations \
         WHERE task_id = ? AND verdict = 'cited' GROUP BY application_policy",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await?;
    for (policy, n) in rows {
        if policy == must_apply {
            counts.must_apply_cited = n;
        } else {
            counts.relevance_cited = n;
        }
    }
    Ok(Some(counts))
}

#[cfg(test)]
mod tests {
    #[test]
    fn split_section_extracts_and_cleans() {
        let out = "[{\"kind\":\"claim\",\"content\":\"x\"}]\nPRAXIS_CITATIONS: {\"cited\":[12],\"uncertain\":[34]}\n";
        let (cleaned, section) = super::split_section(out);
        assert!(!cleaned.contains("PRAXIS_CITATIONS"), "캡처 파싱 입력에서 판정 줄을 걷어낸다");
        assert!(cleaned.contains("claim"), "캡처의 배열 입력은 보존된다");
        assert_eq!(section.as_deref(), Some("{\"cited\":[12],\"uncertain\":[34]}"));
    }

    #[test]
    fn split_section_without_marker_line_is_none() {
        let (cleaned, section) = super::split_section("[]\n그냥 텍스트\n");
        assert!(section.is_none());
        assert!(cleaned.contains("그냥 텍스트"));
    }

    #[test]
    fn marker_match_respects_boundaries() {
        assert!(super::transcript_cites("규칙 M-12를 적용했다", 12));
        assert!(!super::transcript_cites("M-123을 적용", 12)); // M-12는 M-123의 접두가 아니다
        assert!(!super::transcript_cites("ITEM-12 참조", 12)); // 영문 접두 오탐 금지
        assert!(super::transcript_cites("(M-12)", 12));
        assert!(super::transcript_cites("M-123을 적용", 123));
        assert!(!super::transcript_cites("메모리 없음", 12));
    }
}
