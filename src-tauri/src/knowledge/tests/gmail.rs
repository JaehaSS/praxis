//! Gmail 커넥터 — 설정 저장과 자격 증명 격리 검증.

use super::test_pool;
use crate::knowledge::config::{load_gmail, save_gmail, GmailConfig};
use crate::knowledge::source::gmail_api::Message;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

/// **DR-5를 지키는 유일한 장치다.** `knowledge_sources.config`는 DB 안이고, DB 파일은
/// 백업·동기화 폴더·디버깅 덤프로 복사되기 쉽다. 여기에 refresh token이나 client_secret이
/// 섞이면 파일 한 번 유출이 메일함 전체 유출이 된다.
///
/// 조용히 새는 것을 막으려면 "안 넣었다"가 아니라 **"들어가면 실패한다"**로 적어야 한다.
#[tokio::test]
async fn credentials_never_reach_the_database() {
    let pool = test_pool().await;
    save_gmail(
        &pool,
        &GmailConfig {
            query: "newer_than:1y".into(),
            client_id: "1234.apps.googleusercontent.com".into(),
        },
    )
    .await
    .unwrap();

    let (json,): (String,) =
        sqlx::query_as("SELECT COALESCE(config, '') FROM knowledge_sources WHERE id = 'gmail'")
            .fetch_one(&pool)
            .await
            .unwrap();

    for banned in ["secret", "token", "refresh", "password"] {
        assert!(
            !json.to_lowercase().contains(banned),
            "자격 증명이 DB로 샜다 ('{banned}'): {json}"
        );
    }
    // client_id는 공개값이라 config에 있어도 된다 — 오히려 없으면 설정을 못 읽는다.
    assert!(
        json.contains("googleusercontent"),
        "client_id가 저장되지 않았다"
    );
}

/// 저장한 적 없으면 **기본 필터**가 나와야 한다. 비어 있으면 첫 동기화가 메일함 전량을
/// 훑어 25분과 수백 MB를 쓴다 (설계 0020 DR-12).
#[tokio::test]
async fn defaults_narrow_the_scope_before_anything_is_saved() {
    let pool = test_pool().await;
    let cfg = load_gmail(&pool).await.unwrap();

    for category in ["promotions", "social", "forums"] {
        assert!(
            cfg.query.contains(&format!("-category:{category}")),
            "기본 필터가 {category}를 제외하지 않는다: {}",
            cfg.query
        );
    }
    assert!(
        cfg.query.contains("newer_than"),
        "시간창이 없다: {}",
        cfg.query
    );
    assert!(
        cfg.client_id.is_empty(),
        "발급받지 않은 client_id가 채워져 있다"
    );
}

/// 저장 → 로드 왕복. 사용자가 필터를 좁혔는데 다음 동기화에서 기본값으로 되돌아가면
/// 백필이 통째로 다시 돈다.
#[tokio::test]
async fn saved_query_survives_a_round_trip() {
    let pool = test_pool().await;
    let narrowed = "-category:promotions newer_than:6m";
    save_gmail(
        &pool,
        &GmailConfig {
            query: narrowed.into(),
            client_id: "abc.apps.googleusercontent.com".into(),
        },
    )
    .await
    .unwrap();

    let cfg = load_gmail(&pool).await.unwrap();
    assert_eq!(cfg.query, narrowed);
    assert_eq!(cfg.client_id, "abc.apps.googleusercontent.com");
}

/// 자격 증명은 키체인에만 산다. 연결 해제는 **둘 다** 지워야 한다 — 하나라도 남으면
/// "해제했는데 아직 연결돼 있다"가 된다.
#[tokio::test]
async fn credentials_round_trip_through_the_keychain() {
    use crate::knowledge::source::gmail::{CLIENT_SECRET_KEY, REFRESH_TOKEN_KEY};

    crate::secret::set_secret(CLIENT_SECRET_KEY, "s3cr3t")
        .await
        .unwrap();
    crate::secret::set_secret(REFRESH_TOKEN_KEY, "1//refresh")
        .await
        .unwrap();
    assert_eq!(
        crate::secret::get_secret(CLIENT_SECRET_KEY).await.unwrap(),
        Some("s3cr3t".to_string())
    );

    crate::secret::clear_secret(CLIENT_SECRET_KEY)
        .await
        .unwrap();
    crate::secret::clear_secret(REFRESH_TOKEN_KEY)
        .await
        .unwrap();
    assert_eq!(
        crate::secret::get_secret(CLIENT_SECRET_KEY).await.unwrap(),
        None
    );
    assert_eq!(
        crate::secret::get_secret(REFRESH_TOKEN_KEY).await.unwrap(),
        None
    );
}

// ── Task 6: 커서 상태 기계와 문서 변환 ──

use crate::knowledge::source::gmail::{to_document, Cursor};

#[test]
fn cursor_round_trips_through_all_three_states() {
    let states = [
        Cursor::Backfill {
            history_id: "4242".into(),
            page_token: Some("page-2".into()),
        },
        Cursor::Backfill {
            history_id: "4242".into(),
            page_token: None,
        },
        Cursor::History {
            history_id: "9999".into(),
        },
    ];
    for state in states {
        let raw = state.serialize();
        assert_eq!(
            Cursor::parse(raw.as_deref()),
            state,
            "왕복에서 상태가 바뀌었다: {raw:?}"
        );
    }
    assert_eq!(Cursor::Fresh.serialize(), None);
}

/// **백필 시작 시점의 historyId가 커서에 남아야 한다.** 이걸 잃으면 백필이 도는 동안
/// 도착한 메일이 증분 구간에서 통째로 빠진다 — 아무 에러 없이.
#[test]
fn backfill_cursor_carries_the_starting_history_id() {
    let cursor = Cursor::parse(Some("backfill:4242:page-7"));
    match cursor {
        Cursor::Backfill {
            history_id,
            page_token,
        } => {
            assert_eq!(history_id, "4242");
            assert_eq!(page_token.as_deref(), Some("page-7"));
        }
        other => panic!("백필 커서를 해석하지 못했다: {other:?}"),
    }
}

/// pageToken에 ':'가 섞여도 잘리면 안 된다 — 잘린 토큰은 그 페이지부터 조용히 어긋난다.
#[test]
fn page_token_containing_a_colon_survives_parsing() {
    let cursor = Cursor::parse(Some("backfill:4242:tok:with:colons"));
    match cursor {
        Cursor::Backfill { page_token, .. } => {
            assert_eq!(page_token.as_deref(), Some("tok:with:colons"));
        }
        other => panic!("{other:?}"),
    }
}

/// 알 수 없는 커서는 처음부터 다시 한다. 재처리는 `content_hash` 스킵이 흡수하지만,
/// 잘못 해석해 구간을 건너뛰면 누락은 영구적이다.
#[test]
fn unknown_cursor_falls_back_to_a_full_backfill() {
    for raw in [None, Some(""), Some("   "), Some("garbage"), Some("scan-1")] {
        assert_eq!(Cursor::parse(raw), Cursor::Fresh, "입력: {raw:?}");
    }
}

fn message_json(subject: &str, body_b64: &str, internal_date: &str) -> Message {
    serde_json::from_value(serde_json::json!({
        "id": "msg-1",
        "internalDate": internal_date,
        "payload": {
            "mimeType": "text/plain",
            "headers": [{"name": "Subject", "value": subject}],
            "body": {"data": body_b64},
        }
    }))
    .unwrap()
}

#[test]
fn message_becomes_a_document_with_a_usable_link() {
    let body = URL_SAFE_NO_PAD.encode("본문입니다".as_bytes());
    let doc = to_document(&message_json("회의 정리", &body, "1717000000000"));

    assert_eq!(doc.source, "gmail");
    assert_eq!(doc.external_id, "msg-1");
    assert_eq!(doc.kind, "email");
    assert_eq!(doc.title, "회의 정리");
    assert_eq!(doc.body, "본문입니다");
    // 보관처리된 메일도 열려야 하므로 `#inbox/`가 아니라 `#all/`이다.
    assert_eq!(
        doc.url.as_deref(),
        Some("https://mail.google.com/mail/u/0/#all/msg-1")
    );
}

/// 밀리초를 그대로 쓰면 시각이 1000배로 어긋나 정렬과 "최근" 판정이 전부 무너진다.
#[test]
fn internal_date_is_converted_from_milliseconds() {
    let body = URL_SAFE_NO_PAD.encode(b"x");
    let doc = to_document(&message_json("제목", &body, "1717000000000"));
    assert_eq!(doc.updated_at, 1_717_000_000);
}

/// 제목 없는 메일도 색인돼야 한다. 빈 제목이면 `doc_title`이 비어 검색 신호가 하나 준다.
#[test]
fn a_subjectless_message_still_gets_a_title() {
    let body = URL_SAFE_NO_PAD.encode(b"x");
    let doc = to_document(&message_json("   ", &body, "0"));
    assert_eq!(doc.title, "(제목 없음)");
}
