//! 에이전트가 쓴 JSON을 거둬 `quiz_items`에 넣는다 (설계 0044 DR-5).
//!
//! 에이전트에게 DB를 맡기지 않는 이유는 스키마 파손·부분 쓰기 때문이다. 파일이면 검증에
//! 실패했을 때 **그 파일만** 버리면 된다 — 한 배치가 통째로 날아가지 않는다.

use serde::Deserialize;

use super::generate::Kind;

/// 에이전트 출력 한 건. 모르는 필드는 무시한다 — 모델이 설명을 덧붙여도 파싱은 살아야 한다.
#[derive(Debug, Deserialize)]
pub struct RawItem {
    pub kind: String,
    pub question: String,
    #[serde(default)]
    pub choices: Option<Vec<String>>,
    pub answer: String,
    #[serde(default)]
    pub explanation: Option<String>,
    #[serde(default)]
    pub chunk_id: Option<i64>,
}

/// 검증을 통과한 문제. `status`가 여기서 정해진다 — 도메인만 검수를 거친다.
#[derive(Debug, PartialEq, Eq)]
pub struct ValidItem {
    pub kind: Kind,
    pub question: String,
    /// JSON 배열 문자열. 단답형이면 None.
    pub choices: Option<String>,
    pub answer: String,
    pub explanation: Option<String>,
    pub chunk_id: Option<i64>,
    pub status: &'static str,
}

/// 한 건을 검증한다. 하나라도 어긋나면 버린다 — 애매한 문제를 통과시키면 신뢰가 무너진다.
pub fn validate(raw: RawItem) -> Option<ValidItem> {
    let kind = Kind::parse(raw.kind.trim())?;
    let question = raw.question.trim().to_string();
    let answer = raw.answer.trim().to_string();
    if question.is_empty() || answer.is_empty() {
        return None;
    }

    // 도메인 문제는 출처가 없으면 검수할 수 없다 (설계 0044 비즈니스 규칙 2).
    if kind.needs_source() && raw.chunk_id.is_none() {
        return None;
    }
    // 반대로 출처가 필요 없는 종류에 chunk_id가 붙어 오면 무시한다 — 모델이 앞 예시를
    // 그대로 베낀 경우다. CASCADE 대상이 되어 엉뚱하게 지워지는 것을 막는다.
    let chunk_id = if kind.needs_source() {
        raw.chunk_id
    } else {
        None
    };

    let choices = match raw.choices {
        Some(list) if !list.is_empty() => {
            let trimmed: Vec<String> = list.iter().map(|c| c.trim().to_string()).collect();
            if trimmed.iter().any(|c| c.is_empty()) {
                return None;
            }
            // 정답이 보기에 없으면 고를 수가 없다. 모델이 흔히 내는 오류라 반드시 막는다.
            if !trimmed.iter().any(|c| c == &answer) {
                return None;
            }
            Some(serde_json::to_string(&trimmed).ok()?)
        }
        // 빈 배열은 단답형과 같게 취급한다.
        _ => None,
    };

    Some(ValidItem {
        kind,
        question,
        choices,
        answer,
        explanation: raw
            .explanation
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty()),
        chunk_id,
        status: if kind.needs_review() {
            "pending"
        } else {
            "approved"
        },
    })
}

/// 파일 한 개를 판다. **깨진 JSON은 빈 벡터다** — 다른 파일까지 막지 않는다.
///
/// 배열이 아니라 객체 하나만 온 경우도 받는다. 모델이 한 건일 때 배열을 생략하는 일이 잦다.
pub fn parse_file(body: &str) -> Vec<ValidItem> {
    let raws: Vec<RawItem> = match serde_json::from_str::<Vec<RawItem>>(body) {
        Ok(list) => list,
        Err(_) => match serde_json::from_str::<RawItem>(body) {
            Ok(one) => vec![one],
            Err(_) => return Vec::new(),
        },
    };
    raws.into_iter().filter_map(validate).collect()
}

/// inbox를 비워 DB에 넣고, 새로 들어간 문제 수를 돌려준다.
///
/// 처리한 파일은 지우지 않고 `done/`으로 옮긴다 — 생성이 왜 빈손으로 끝났는지 알아낼
/// 유일한 흔적이다.
///
/// `now`를 인자로 받는 이유는 테스트 결정성 때문이다.
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
            continue; // 읽기 실패한 파일은 다음 틱에 다시 시도한다.
        };
        for item in parse_file(&body) {
            if store(pool, &item, now).await? {
                stored += 1;
            }
        }
        // 이동 실패는 진행을 막지 않는다 — 다음 틱에 같은 파일을 다시 읽지만
        // 중복 질문은 store가 걸러낸다.
        let _ = std::fs::create_dir_all(&done);
        if let Some(name) = path.file_name() {
            let _ = std::fs::rename(&path, done.join(name));
        }
    }
    Ok(stored)
}

/// 한 건을 넣는다. 이미 같은 질문이 있으면 넣지 않고 `false`를 돌려준다.
///
/// 임베딩 유사도까지 보지 않는 이유가 있다. (a) 같은 프롬프트로 만들면 표현이 거의 같아
/// 정확 일치로 대부분 걸리고, (b) `embed`는 130MB 모델 로드가 필요한 데다 실패 시
/// 통과시키는 best-effort라 중복 방어선으로 삼기에 약하다. 필요해지면 그때 얹는다.
async fn store(pool: &sqlx::SqlitePool, item: &ValidItem, now: i64) -> anyhow::Result<bool> {
    let (existing,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM quiz_items WHERE question = ?")
        .bind(&item.question)
        .fetch_one(pool)
        .await?;
    if existing > 0 {
        return Ok(false);
    }
    sqlx::query(
        "INSERT INTO quiz_items \
         (kind, question, choices, answer, explanation, chunk_id, source_excerpt, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, \
            (SELECT content FROM knowledge_chunks WHERE id = ?), ?, ?)",
    )
    .bind(item.kind.as_str())
    .bind(&item.question)
    .bind(&item.choices)
    .bind(&item.answer)
    .bind(&item.explanation)
    .bind(item.chunk_id)
    // 검수 시점의 근거를 복제해 둔다 — 청크가 나중에 바뀌어도 무엇을 보고 승인했는지 남는다.
    .bind(item.chunk_id)
    .bind(item.status)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{parse_file, validate, RawItem};
    use crate::quiz::generate::Kind;

    fn raw(kind: &str, chunk_id: Option<i64>) -> RawItem {
        RawItem {
            kind: kind.into(),
            question: "질문".into(),
            choices: Some(vec!["A".into(), "B".into()]),
            answer: "A".into(),
            explanation: Some("설명".into()),
            chunk_id,
        }
    }

    #[test]
    fn domain_without_a_source_is_rejected() {
        assert!(validate(raw("domain", None)).is_none());
    }

    #[test]
    fn domain_with_a_source_is_pending() {
        let item = validate(raw("domain", Some(7))).unwrap();
        assert_eq!(item.status, "pending");
        assert_eq!(item.chunk_id, Some(7));
    }

    /// 일반 종류는 검수 없이 바로 출제된다 (설계 0044 DR-4).
    #[test]
    fn general_kinds_are_approved_immediately() {
        let item = validate(raw("vocab", None)).unwrap();
        assert_eq!(item.status, "approved");
        assert_eq!(item.kind, Kind::Vocab);
    }

    /// 출처가 필요 없는 종류에 붙어 온 chunk_id는 떨군다 — 안 그러면 청크 삭제에
    /// 엉뚱한 문제가 CASCADE로 딸려 지워진다.
    #[test]
    fn chunk_id_is_dropped_for_sourceless_kinds() {
        let item = validate(raw("trivia", Some(7))).unwrap();
        assert_eq!(item.chunk_id, None);
    }

    /// 보기에 없는 정답은 고를 수가 없다.
    #[test]
    fn answer_must_appear_in_the_choices() {
        let mut r = raw("vocab", None);
        r.answer = "C".into();
        assert!(validate(r).is_none());
    }

    #[test]
    fn blank_fields_are_rejected() {
        let mut r = raw("vocab", None);
        r.question = "   ".into();
        assert!(validate(r).is_none());

        let mut r = raw("vocab", None);
        r.answer = "".into();
        assert!(validate(r).is_none());
    }

    #[test]
    fn unknown_kind_is_rejected() {
        assert!(validate(raw("게임", None)).is_none());
    }

    /// 보기가 없으면 단답형이다.
    #[test]
    fn missing_choices_means_short_answer() {
        let mut r = raw("vocab", None);
        r.choices = None;
        assert_eq!(validate(r).unwrap().choices, None);

        let mut r = raw("vocab", None);
        r.choices = Some(vec![]);
        assert_eq!(validate(r).unwrap().choices, None);
    }

    #[test]
    fn broken_json_yields_nothing_instead_of_panicking() {
        assert!(parse_file("{").is_empty());
        assert!(parse_file("").is_empty());
        assert!(parse_file("null").is_empty());
    }

    /// 한 건이면 배열을 생략하는 모델이 있다.
    #[test]
    fn a_bare_object_is_accepted() {
        let body = r#"{"kind":"vocab","question":"epistemic?","answer":"인식론적"}"#;
        assert_eq!(parse_file(body).len(), 1);
    }

    /// 한 건이 틀렸다고 나머지를 버리지 않는다.
    #[test]
    fn invalid_entries_are_skipped_not_fatal() {
        let body = r#"[
          {"kind":"vocab","question":"q1","answer":"a1"},
          {"kind":"domain","question":"q2","answer":"a2"},
          {"kind":"trivia","question":"q3","answer":"a3"}
        ]"#;
        let items = parse_file(body);
        assert_eq!(items.len(), 2, "출처 없는 domain만 빠져야 한다");
        assert_eq!(items[0].question, "q1");
        assert_eq!(items[1].question, "q3");
    }
}
