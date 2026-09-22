//! 에이전트가 쓴 JSON을 거둬 `retro_digests`에 넣는다 (설계 0054 DR-5).
//!
//! 퀴즈(`quiz/inbox.rs`)와 같은 이유로 파일을 거친다 — 에이전트에게 DB를 맡기면 스키마
//! 파손·부분 쓰기를 막을 수 없고, 파일이면 검증에 실패했을 때 **그 파일만** 버리면 된다.
//!
//! **에이전트가 준 수치는 받지 않는다.** 받는 것은 `week_start`와 `body`뿐이고, `facts`는
//! 적재 시점에 DB에서 다시 계산한다. 프롬프트에 넣어 준 값을 그대로 되돌려받아 저장하면
//! 검증이 자기 자신을 검증하는 꼴이 된다(DR-7).

use serde::Deserialize;

/// 에이전트 출력. 모르는 필드는 무시한다 — 모델이 설명을 덧붙여도 파싱은 살아야 한다.
#[derive(Debug, Deserialize)]
pub struct RawDigest {
    pub week_start: i64,
    pub body: String,
}

/// 검증을 통과한 다이제스트.
#[derive(Debug, PartialEq)]
pub struct ValidDigest {
    pub week_start: i64,
    pub body: String,
}

/// 최소 길이. 이보다 짧으면 서술이 아니라 사고다.
const MIN_BODY_CHARS: usize = 40;

pub fn validate(raw: RawDigest) -> Option<ValidDigest> {
    let body = raw.body.trim().to_string();
    if raw.week_start <= 0 || body.chars().count() < MIN_BODY_CHARS {
        return None;
    }
    Some(ValidDigest {
        week_start: raw.week_start,
        body,
    })
}

/// 객체 하나 또는 배열 둘 다 받는다 — 모델이 배열로 감싸는 일이 흔하다.
pub fn parse_file(body: &str) -> Vec<ValidDigest> {
    match serde_json::from_str::<RawDigest>(body) {
        Ok(one) => validate(one).into_iter().collect(),
        Err(_) => match serde_json::from_str::<Vec<RawDigest>>(body) {
            Ok(list) => list.into_iter().filter_map(validate).collect(),
            Err(_) => Vec::new(),
        },
    }
}

/// inbox를 거둬 DB에 넣고, 새로 들어간 다이제스트 수를 돌려준다.
///
/// 처리한 파일은 지우지 않고 `done/`으로 옮긴다 — 생성이 왜 빈손으로 끝났는지 알아낼
/// 유일한 흔적이다(퀴즈와 같은 규약).
pub async fn collect_and_store(
    pool: &sqlx::SqlitePool,
    dir: &std::path::Path,
    now: i64,
) -> anyhow::Result<usize> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(0); // inbox가 아직 없다 — 생성이 한 번도 안 돌았다.
    };
    let done = dir.join("done");
    let mut stored = 0usize;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue; // 읽기 실패는 다음 호출에 다시 시도한다.
        };
        for digest in parse_file(&body) {
            if store(pool, &digest, now).await? {
                stored += 1;
            }
        }
        let _ = std::fs::create_dir_all(&done);
        if let Some(name) = path.file_name() {
            let _ = std::fs::rename(&path, done.join(name));
        }
    }
    Ok(stored)
}

/// 한 건을 넣는다. 그 주가 이미 있으면 넣지 않고 `false`를 돌려준다.
///
/// **덮어쓰지 않는다.** 같은 주에 서술이 둘이면 어느 쪽이 맞는지 알 수 없다 — 다시 만들려면
/// 사람이 지운다(설계 0054 §6.3).
pub async fn store(
    pool: &sqlx::SqlitePool,
    digest: &ValidDigest,
    now: i64,
) -> anyhow::Result<bool> {
    // 수치는 에이전트가 준 것이 아니라 여기서 다시 만든다.
    let facts = super::collect_facts(pool, digest.week_start).await?;
    let facts_json = serde_json::to_string(&facts)?;

    let affected = sqlx::query(
        "INSERT OR IGNORE INTO retro_digests \
           (week_start, body, facts, agent, model, generated_at) \
         VALUES (?, ?, ?, NULL, NULL, ?)",
    )
    .bind(digest.week_start)
    .bind(&digest.body)
    .bind(facts_json)
    .bind(now)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: &str = "이번 주에는 작업 23건 중 17건이 승인됐고 폐기율은 지난주보다 낮아졌다.";

    #[test]
    fn accepts_object_and_array() {
        let one = format!(r#"{{"week_start":100,"body":"{LONG}"}}"#);
        assert_eq!(parse_file(&one).len(), 1);
        let many = format!(r#"[{{"week_start":100,"body":"{LONG}"}}]"#);
        assert_eq!(parse_file(&many).len(), 1);
    }

    #[test]
    fn ignores_unknown_fields() {
        let body = format!(r#"{{"week_start":100,"body":"{LONG}","note":"설명을 덧붙였다"}}"#);
        assert_eq!(parse_file(&body).len(), 1);
    }

    /// 깨진 JSON이 패닉을 내면 그 배치 전체가 날아간다.
    #[test]
    fn broken_json_yields_nothing() {
        assert!(parse_file("{not json").is_empty());
        assert!(parse_file("").is_empty());
        assert!(parse_file("[]").is_empty());
    }

    #[test]
    fn rejects_too_short_or_bad_week() {
        assert!(parse_file(r#"{"week_start":100,"body":"짧다"}"#).is_empty());
        let body = format!(r#"{{"week_start":0,"body":"{LONG}"}}"#);
        assert!(parse_file(&body).is_empty());
    }
}
