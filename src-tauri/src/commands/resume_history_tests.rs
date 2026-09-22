//! 이어받은 대화의 **이력 조립** — 물려받은 이벤트가 무엇으로 표시되고 어느 순서로 오는지.
//!
//! 조립이 틀리면 화면과 벤더의 기억이 어긋난다. 벤더 세션은 이미 옛 대화를 이어가고 있으므로,
//! 화면이 그 대화를 보여주지 못하면 사용자는 자기가 하지 않은 말을 근거로 답하는 에이전트를
//! 본다. 그 어긋남은 조용히 일어나고 로그에 아무것도 남기지 않는다 — 그래서 여기서 잡는다.
//!
//! `commands`가 크레이트 밖에 보이지 않아 `tests/`가 아니라 여기 산다.

use super::*;

async fn pool(name: &str) -> SqlitePool {
    let path = crate::testtmp::dir().join(format!("commands-resume-{name}.sqlite"));
    db::init_pool(path.to_str().unwrap()).await.unwrap()
}

async fn conversation_task(pool: &SqlitePool, instruction: &str) -> i64 {
    db::insert_task(
        pool,
        "/repo",
        "praxis/resume",
        "main",
        "/repo/.praxis/wt/resume",
        instruction,
        Some("claude"),
        None,
        "conversation",
        1000,
    )
    .await
    .unwrap()
}

/// `texts` 순서대로 사용자 이벤트를 쌓는다.
async fn say(pool: &SqlitePool, id: i64, texts: &[&str]) {
    for (n, text) in texts.iter().enumerate() {
        db::append_convo_event(
            pool,
            id,
            &serde_json::json!({ "kind": "user", "text": text }).to_string(),
            1000 + n as i64,
        )
        .await
        .unwrap();
    }
}

fn kind(event: &serde_json::Value) -> &str {
    event["kind"].as_str().unwrap_or("")
}

/// 이어받기가 없으면 붙일 것도 없다 — 빈 vec여야 뒤의 `extend`가 자기 이력만 남긴다.
#[tokio::test]
async fn a_fresh_conversation_inherits_nothing() {
    let pool = pool("fresh").await;
    let id = conversation_task(&pool, "새 대화").await;
    say(&pool, id, &["안녕"]).await;

    let items = inherited_convo_history(&pool, id).await.expect("조립");
    assert!(items.is_empty(), "체인이 없으면 승계분도 없다: {items:?}");
}

/// 승계분의 모든 이벤트에 `inherited: true`와 **실제 출처 id**가 박혀야 한다.
///
/// 이 플래그가 빠지면 프런트는 남의 이벤트를 자기 것으로 본다. 되감기처럼 이벤트에 걸리는
/// 동작을 지금 작업 id로 실행하면 엉뚱한 워크트리를 건드린다 — 플래그가 그 동작의 잠금 근거다.
/// 경계 이벤트가 원본당 정확히 하나인 것도 같은 이유다: 둘이면 화면에 없는 대화 구분선이 생기고,
/// 없으면 두 대화가 한 덩어리로 붙어 어디까지가 옛 대화인지 알 수 없다.
#[tokio::test]
async fn inherited_events_are_stamped_and_each_origin_gets_one_boundary() {
    let pool = pool("stamp").await;
    let a = conversation_task(&pool, "A").await;
    let b = conversation_task(&pool, "B").await;
    let c = conversation_task(&pool, "C").await;
    say(&pool, a, &["a1", "a2"]).await;
    say(&pool, b, &["b1"]).await;
    say(&pool, c, &["c1"]).await;
    db::adopt_conversation(&pool, b, a, 2000).await.unwrap();
    db::adopt_conversation(&pool, c, b, 2001).await.unwrap();

    let items = inherited_convo_history(&pool, c).await.expect("조립");

    // 오래된 원본부터: A의 이벤트 → A 경계 → B의 이벤트 → B 경계. C 자신은 여기 없다.
    let shape: Vec<&str> = items.iter().map(kind).collect();
    assert_eq!(
        shape,
        vec!["user", "user", "resumed_from", "user", "resumed_from"]
    );
    let texts: Vec<&str> = items
        .iter()
        .filter_map(|event| event["text"].as_str())
        .collect();
    assert_eq!(texts, vec!["a1", "a2", "b1"], "시간순이어야 대화가 읽힌다");

    assert!(
        items
            .iter()
            .all(|event| event["inherited"] == serde_json::Value::Bool(true)),
        "경계까지 포함해 전부 남의 것으로 표시된다: {items:?}"
    );
    let sources: Vec<i64> = items
        .iter()
        .map(|event| event["source_task_id"].as_i64().expect("출처 id"))
        .collect();
    assert_eq!(sources, vec![a, a, a, b, b], "출처는 그 이벤트를 쓴 작업이다");

    let boundaries: Vec<i64> = items
        .iter()
        .filter(|event| kind(event) == "resumed_from")
        .map(|event| event["source_task_id"].as_i64().unwrap())
        .collect();
    assert_eq!(boundaries, vec![a, b], "원본마다 경계 하나씩");
}

/// 승계분이 **자기 이벤트보다 앞에** 온다 — 순서가 이 기능의 요점이다.
///
/// 뒤에 붙이면 지금 턴이 옛 대화보다 먼저 일어난 것으로 읽힌다. 대화창은 위에서 아래로
/// 시간이 흐른다는 약속 위에 서 있고, 그 약속이 깨지면 화면은 읽을 수 없는 것이 된다.
#[tokio::test]
async fn inherited_history_precedes_the_tasks_own_events() {
    let pool = pool("order").await;
    let source = conversation_task(&pool, "원본").await;
    let heir = conversation_task(&pool, "이어받기").await;
    say(&pool, source, &["옛 질문"]).await;
    say(&pool, heir, &["새 질문"]).await;
    db::adopt_conversation(&pool, heir, source, 2000).await.unwrap();

    // `convo_history`가 하는 병합과 같은 조립 — 승계분이 바탕이고 자기 이력이 뒤에 붙는다.
    let mut items = inherited_convo_history(&pool, heir).await.expect("조립");
    let own = db::list_convo_events(&pool, heir).await.unwrap();
    items.extend(own.iter().filter_map(|row| serde_json::from_str(row).ok()));

    let texts: Vec<&str> = items
        .iter()
        .filter_map(|event| event["text"].as_str())
        .collect();
    assert_eq!(texts, vec!["옛 질문", "새 질문"]);
    let boundary = items
        .iter()
        .position(|event| kind(event) == "resumed_from")
        .expect("경계");
    let mine = items
        .iter()
        .position(|event| event["text"] == "새 질문")
        .expect("자기 이벤트");
    assert!(boundary < mine, "경계는 자기 이력 앞에 선다");
    assert!(
        items[mine].get("inherited").is_none(),
        "자기 이벤트에는 승계 표시가 붙지 않는다"
    );
}

/// 깨진 행 하나가 나머지 이력을 삼키지 않는다.
///
/// `convo_events`는 자유 텍스트 칸이고 과거 형식이 섞여 들어올 수 있다. 한 줄 때문에
/// 조립이 실패하면 이어받은 대화창 전체가 빈 화면이 된다 — 건너뛰는 쪽이 옳다.
#[tokio::test]
async fn a_malformed_row_is_skipped_without_losing_the_rest() {
    let pool = pool("malformed").await;
    let source = conversation_task(&pool, "원본").await;
    let heir = conversation_task(&pool, "이어받기").await;
    say(&pool, source, &["앞"]).await;
    db::append_convo_event(&pool, source, "{깨진 JSON", 1500)
        .await
        .unwrap();
    say(&pool, source, &["뒤"]).await;
    db::adopt_conversation(&pool, heir, source, 2000).await.unwrap();

    let items = inherited_convo_history(&pool, heir).await.expect("조립");
    let texts: Vec<&str> = items
        .iter()
        .filter_map(|event| event["text"].as_str())
        .collect();
    assert_eq!(texts, vec!["앞", "뒤"]);
    assert_eq!(items.len(), 3, "깨진 행만 빠지고 경계는 그대로");
}

/// 상한을 넘으면 **오래된 쪽부터** 버리고 잘렸다는 사실을 맨 앞에 남긴다.
///
/// 직전 대화의 끝이 지금 이어가는 맥락이므로 가까운 과거를 지키는 것이 전부다. 표시가 없으면
/// 사용자는 사라진 이력을 버그로 읽고, 잘린 자리에서 에이전트가 아는 것을 설명할 수 없다.
///
/// 상한이 세는 것은 **원장에서 읽어 온 실제 이벤트**다. 경계 구분선은 합성물이고 체인 깊이
/// 상한(32)만큼만 생기므로 예산에서 빼지 않는다 — 그래야 `dropped`가 "못 보게 된 메시지 수"와
/// 정확히 같아진다. 구분선까지 예산에 넣으면 그 수가 메시지도 구분선도 아닌 값이 된다.
#[tokio::test]
async fn overflowing_history_drops_the_oldest_and_says_so() {
    let pool = pool("truncate").await;
    let old = conversation_task(&pool, "먼 과거").await;
    let recent = conversation_task(&pool, "직전").await;
    let heir = conversation_task(&pool, "이어받기").await;

    // 두 원본 합쳐 실제 이벤트 2100건 — 상한을 100건 넘긴다.
    let mut tx = pool.begin().await.unwrap();
    for (id, count, tag) in [(old, 1_500, "old"), (recent, 600, "recent")] {
        for n in 0..count {
            sqlx::query("INSERT INTO convo_events (task_id, ts, event) VALUES (?, ?, ?)")
                .bind(id)
                .bind(1000 + n)
                .bind(
                    serde_json::json!({ "kind": "user", "text": format!("{tag}-{n}") }).to_string(),
                )
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    tx.commit().await.unwrap();
    db::adopt_conversation(&pool, recent, old, 2000).await.unwrap();
    db::adopt_conversation(&pool, heir, recent, 2001).await.unwrap();

    let items = inherited_convo_history(&pool, heir).await.expect("조립");

    assert_eq!(kind(&items[0]), "history_truncated", "잘렸다는 표시가 맨 앞");
    assert_eq!(items[0]["dropped"], 100);
    assert_eq!(items[0]["inherited"], serde_json::Value::Bool(true));
    assert_eq!(
        items.len(),
        INHERITED_HISTORY_MAX + 3,
        "표시 한 줄 + 상한 + 원본 둘의 경계"
    );
    assert_eq!(
        items
            .iter()
            .filter(|event| kind(event) == "user")
            .count(),
        INHERITED_HISTORY_MAX,
        "읽어 온 실제 이벤트는 정확히 상한만큼"
    );

    // 버려진 것은 가장 먼 과거뿐이다 — 남은 첫 이벤트가 그 경계를 보여준다.
    assert_eq!(items[1]["text"], "old-100");
    assert!(
        !items.iter().any(|event| event["text"] == "old-99"),
        "상한 밖의 먼 과거는 남지 않는다"
    );
    // 직전 대화는 한 건도 잃지 않는다.
    assert_eq!(
        items
            .iter()
            .filter(|event| event["source_task_id"].as_i64() == Some(recent)
                && kind(event) == "user")
            .count(),
        600
    );
    assert_eq!(
        kind(items.last().unwrap()),
        "resumed_from",
        "가장 최근 원본의 경계가 자기 이력 바로 앞에 선다"
    );
}
