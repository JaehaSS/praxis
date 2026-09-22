use sqlx::SqlitePool;

use crate::memory::{self, tier};

const DEDUPE_COSINE_THRESHOLD: f32 = 0.92;

/// 추출된 메모리가 인젝션 페이로드로 보이는지 판정한다.
pub(super) fn looks_injected(content: &str) -> bool {
    let candidate = content.trim();
    candidate.chars().count() > 300
        || candidate.starts_with('#')
        || (candidate.contains('<') && candidate.contains('>'))
        || candidate.contains("]\n")
        || candidate.to_lowercase().contains("ignore the above")
        || candidate.contains("무시하")
}

/// 같은 project scope의 기존 지식과 중복인지 판정한다.
pub(super) async fn is_duplicate(
    pool: &SqlitePool,
    repository: &str,
    content: &str,
    embedding: Option<&[f32]>,
) -> anyhow::Result<bool> {
    if let Some(vector) = embedding {
        let best = memory::max_project_cosine(pool, repository, vector).await?;
        return Ok(best >= DEDUPE_COSINE_THRESHOLD);
    }
    let existing: Vec<String> =
        sqlx::query_scalar("SELECT content FROM memories WHERE tier = ? AND scope_key = ?")
            .bind(tier::PROJECT)
            .bind(repository)
            .fetch_all(pool)
            .await?;
    let normalized = normalize_for_compare(content);
    Ok(existing
        .iter()
        .any(|item| normalize_for_compare(item) == normalized))
}

fn normalize_for_compare(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
