//! Gmail 응답 파싱(Task 4)과 본문 정규화(Task 5) 검증.
//!
//! 네트워크를 타지 않는다. 실제 Gmail 응답 형태를 픽스처로 박아 **파싱과 정규화만**
//! 고정한다 — 이 둘이 틀리면 증상이 "메일이 색인은 됐는데 검색이 이상함"이라 늦게 발견된다.

use crate::knowledge::normalize::{extract_body, message_body, strip_quoted};
use crate::knowledge::source::gmail_api::{
    backoff_delay, should_retry, HistoryList, Message, MessageList, Profile,
};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

fn b64(text: &str) -> String {
    URL_SAFE_NO_PAD.encode(text.as_bytes())
}

// ── Task 4: 응답 파싱 ──

#[test]
fn message_list_parses_and_carries_the_page_token() {
    let json = r#"{"messages":[{"id":"m1","threadId":"t1"},{"id":"m2","threadId":"t2"}],
                   "nextPageToken":"page-2","resultSizeEstimate":2}"#;
    let list: MessageList = serde_json::from_str(json).unwrap();
    assert_eq!(list.messages.len(), 2);
    assert_eq!(list.messages[0].id, "m1");
    assert_eq!(list.next_page_token.as_deref(), Some("page-2"));
}

/// 마지막 페이지에는 `nextPageToken`이 아예 없다. 없는 것을 에러로 만들면
/// 백필이 끝나는 순간 실패한다.
#[test]
fn message_list_tolerates_a_missing_page_token() {
    let list: MessageList = serde_json::from_str(r#"{"messages":[{"id":"m1"}]}"#).unwrap();
    assert!(list.next_page_token.is_none());
}

/// `internalDate`는 **문자열**이다. 숫자로 역직렬화하면 메시지 파싱이 통째로 실패한다.
#[test]
fn internal_date_arrives_as_a_string() {
    let json = r#"{"id":"m1","internalDate":"1717000000000","labelIds":["INBOX"]}"#;
    let message: Message = serde_json::from_str(json).unwrap();
    assert_eq!(message.internal_date, "1717000000000");
    assert_eq!(message.label_ids, vec!["INBOX"]);
}

#[test]
fn history_parses_added_and_deleted() {
    let json = r#"{"history":[{"id":"1","messagesAdded":[{"message":{"id":"new1"}}],
                    "messagesDeleted":[{"message":{"id":"gone1"}}]}],
                   "historyId":"999"}"#;
    let history: HistoryList = serde_json::from_str(json).unwrap();
    assert_eq!(history.history[0].messages_added[0].message.id, "new1");
    assert_eq!(history.history[0].messages_deleted[0].message.id, "gone1");
    assert_eq!(history.history_id, "999");
}

/// 변경이 없으면 `history` 키 자체가 오지 않는다.
#[test]
fn empty_history_is_not_an_error() {
    let history: HistoryList = serde_json::from_str(r#"{"historyId":"999"}"#).unwrap();
    assert!(history.history.is_empty());
}

#[test]
fn profile_carries_the_history_id() {
    let json = r#"{"emailAddress":"a@b.c","messagesTotal":10,"threadsTotal":5,"historyId":"4242"}"#;
    let profile: Profile = serde_json::from_str(json).unwrap();
    assert_eq!(profile.history_id, "4242");
}

/// 할당량 초과도 403, 권한 없음도 403이다. 전자는 기다리면 풀리고 후자는 영원히
/// 안 풀린다 — 구분하지 않으면 잘못된 scope로 연결한 사용자가 무한 재시도한다.
#[test]
fn retry_policy_splits_the_two_kinds_of_403() {
    assert!(should_retry(403, r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#));
    assert!(!should_retry(403, r#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#));
    assert!(should_retry(429, ""));
    assert!(should_retry(503, ""));
    assert!(!should_retry(404, ""));
    assert!(!should_retry(400, ""));
}

/// 분당 할당량은 1분이면 회복된다. 그보다 오래 기다리는 것은 사용자를 붙잡아 둘 뿐이다.
#[test]
fn backoff_grows_but_stays_bounded() {
    assert!(backoff_delay(0) < backoff_delay(2));
    assert!(
        backoff_delay(20) <= std::time::Duration::from_secs(32),
        "백오프 상한이 없다: {:?}",
        backoff_delay(20)
    );
}

// ── Task 5: 본문 정규화 ──

fn part(mime: &str, body: &str) -> crate::knowledge::source::gmail_api::MessagePart {
    serde_json::from_value(serde_json::json!({
        "mimeType": mime,
        "body": { "data": b64(body) },
    }))
    .unwrap()
}

fn multipart(
    children: Vec<crate::knowledge::source::gmail_api::MessagePart>,
) -> crate::knowledge::source::gmail_api::MessagePart {
    serde_json::from_value(serde_json::json!({
        "mimeType": "multipart/alternative",
        "parts": children.iter().map(|c| serde_json::to_value(SerPart(c)).unwrap()).collect::<Vec<_>>(),
    }))
    .unwrap()
}

/// `MessagePart`는 Deserialize만 파생돼 있어(응답 전용) 테스트에서 직렬화하려면
/// 얇은 어댑터가 필요하다. 프로덕션 타입에 Serialize를 붙이지 않는 이유는
/// **우리가 이걸 보내는 일이 없기** 때문이다 — 읽기 전용 커넥터다.
struct SerPart<'a>(&'a crate::knowledge::source::gmail_api::MessagePart);

impl serde::Serialize for SerPart<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("mimeType", &self.0.mime_type)?;
        if let Some(body) = &self.0.body {
            map.serialize_entry("body", &serde_json::json!({ "data": body.data }))?;
        }
        map.end()
    }
}

#[test]
fn plain_text_wins_over_html() {
    let payload = multipart(vec![
        part("text/plain", "사람이 쓴 본문"),
        part("text/html", "<p>HTML 버전</p>"),
    ]);
    assert_eq!(extract_body(&payload), "사람이 쓴 본문");
}

/// plain이 없을 때만 html로 내려간다. 태그가 그대로 색인되면 검색어가 마크업에 걸린다.
#[test]
fn html_is_flattened_when_plain_is_absent() {
    let payload = multipart(vec![part(
        "text/html",
        "<div><p>안녕하세요</p><p>본문입니다</p></div>",
    )]);
    let body = extract_body(&payload);
    assert!(body.contains("안녕하세요"), "본문이 사라졌다: {body}");
    assert!(!body.contains('<'), "태그가 남았다: {body}");
}

#[test]
fn quoted_reply_is_cut_at_the_attribution_line() {
    let body = "확인했습니다. 반영할게요.\n\nOn Wed, Aug 6, 2026 at 9:00 AM 홍길동 wrote:\n> 원문입니다\n> 두 번째 줄";
    assert_eq!(strip_quoted(body), "확인했습니다. 반영할게요.");
}

/// 한국어 Gmail의 인용 머리글. 영어 형태만 처리하면 국내 메일이 통째로 인용문을 안고 간다.
#[test]
fn korean_attribution_line_is_recognized() {
    let body = "네 좋습니다.\n\n2026년 8월 6일 (수) 오전 9:00, 홍길동 <a@b.c>님이 작성:\n> 원문";
    assert_eq!(strip_quoted(body), "네 좋습니다.");
}

#[test]
fn forwarded_block_and_signature_are_dropped() {
    let forwarded = "전달합니다\n\n---------- Forwarded message ----------\nFrom: a@b.c";
    assert_eq!(strip_quoted(forwarded), "전달합니다");

    let signed = "본문\n\n-- \n홍길동\n010-0000-0000";
    assert_eq!(strip_quoted(signed), "본문");
}

/// **안전망.** 인용만으로 이뤄진 메일(코멘트 없이 전달만 한 경우)에서 색인이 통째로
/// 사라지면 그 메일은 영영 검색되지 않는다. 노이즈가 남는 편이 낫다.
#[test]
fn a_message_that_is_only_quotation_keeps_its_body() {
    let body = "> 전부 인용문입니다\n> 두 번째 줄";
    let result = strip_quoted(body);
    assert!(!result.is_empty(), "인용만 있는 메일의 본문이 통째로 사라졌다");
    assert!(result.contains("인용문"), "{result}");
}

#[test]
fn message_body_decodes_and_strips_in_one_pass() {
    let payload = multipart(vec![part(
        "text/plain",
        "회신 본문\n\nOn Wed, Aug 6, 2026 at 9:00 AM 아무개 wrote:\n> 인용",
    )]);
    assert_eq!(message_body(&payload), "회신 본문");
}
