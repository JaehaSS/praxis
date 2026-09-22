//! Gmail REST 응답 타입과 호출 래퍼 (플랜 0028 Task 4).
//!
//! **할당량이 이 모듈의 설계 제약이다.** `users.messages.get`은 건당 5 quota unit이라
//! 3만 통 백필은 분당 제한에 반드시 걸린다(설계 0020 §9 병목 2위). 백오프는 나중에
//! 붙이는 최적화가 아니라 처음부터 있어야 하는 전제다.

use serde::de::DeserializeOwned;
use serde::Deserialize;

const API_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

/// 한 번에 가져올 메시지 수. 페이지가 곧 재개 단위이므로(플랜 0028 DR-C)
/// 너무 크면 중단 시 잃는 일이 많아지고, 너무 작으면 커밋 오버헤드가 는다.
pub const PAGE_SIZE: u32 = 100;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageList {
    #[serde(default)]
    pub messages: Vec<MessageRef>,
    #[serde(default)]
    pub next_page_token: Option<String>,
    /// Gmail이 주는 **어림수**다. 정확한 개수가 아니라 규모 감각을 주는 값이라,
    /// 백필 전에 "얼마나 들어올지"를 보여주는 용도로만 쓴다 (DR-12).
    #[serde(default)]
    pub result_size_estimate: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageRef {
    pub id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    /// **문자열이다.** Gmail은 밀리초 epoch를 JSON 문자열로 준다 — 숫자로 역직렬화하면
    /// 통째로 실패한다.
    #[serde(default)]
    pub internal_date: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
    #[serde(default)]
    pub payload: Option<MessagePart>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePart {
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub headers: Vec<Header>,
    #[serde(default)]
    pub body: Option<PartBody>,
    /// multipart면 여기에 하위 파트가 재귀로 들어온다.
    #[serde(default)]
    pub parts: Vec<MessagePart>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct PartBody {
    /// base64url. 첨부는 여기 대신 `attachmentId`가 오는데, 첨부는 색인하지 않으므로 무시한다.
    #[serde(default)]
    pub data: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(default)]
    pub email_address: String,
    #[serde(default)]
    pub history_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryList {
    #[serde(default)]
    pub history: Vec<HistoryRecord>,
    #[serde(default)]
    pub next_page_token: Option<String>,
    /// 이번 응답 기준 최신 historyId. 다음 증분의 시작점이 된다.
    #[serde(default)]
    pub history_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    #[serde(default)]
    pub messages_added: Vec<HistoryMessage>,
    #[serde(default)]
    pub messages_deleted: Vec<HistoryMessage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HistoryMessage {
    pub message: MessageRef,
}

impl MessagePart {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }
}

/// 재시도해야 하는 실패인가.
///
/// **`403`은 body를 봐야 갈린다.** 할당량 초과도 403이고 권한 없음도 403인데,
/// 전자는 기다리면 풀리고 후자는 영원히 안 풀린다. 구분하지 않으면 잘못된 scope로
/// 연결한 사용자가 무한히 재시도하는 것을 지켜보게 된다.
pub fn should_retry(status: u16, body: &str) -> bool {
    if status == 429 || status >= 500 {
        return true;
    }
    if status == 403 {
        return body.contains("rateLimitExceeded")
            || body.contains("userRateLimitExceeded")
            || body.contains("backendError");
    }
    false
}

/// 지수 백오프. 시도 횟수만 받는 순수 함수라 테스트로 상한을 고정할 수 있다.
pub fn backoff_delay(attempt: u32) -> std::time::Duration {
    // 1s, 2s, 4s, 8s, 16s … 32s에서 멈춘다. 분당 할당량은 1분이면 회복되므로
    // 그 이상 기다리는 것은 사용자를 붙잡아 두기만 한다.
    let seconds = 1u64 << attempt.min(5);
    std::time::Duration::from_secs(seconds.min(32))
}

const MAX_ATTEMPTS: u32 = 6;

/// GET + 백오프 재시도. 4xx(할당량 제외)는 즉시 실패시킨다 — 재시도해도 같다.
async fn get_json<T: DeserializeOwned>(
    http: &reqwest::Client,
    access_token: &str,
    url: reqwest::Url,
) -> anyhow::Result<T> {
    let mut attempt = 0;
    loop {
        let response = http
            .get(url.clone())
            .bearer_auth(access_token)
            .send()
            .await?;
        let status = response.status().as_u16();
        let body = response.text().await?;

        if status == 200 {
            return serde_json::from_str(&body)
                .map_err(|e| anyhow::anyhow!("Gmail 응답 파싱 실패({url}): {e}: {body}"));
        }
        if !should_retry(status, &body) || attempt >= MAX_ATTEMPTS {
            return Err(ApiError {
                status,
                body,
                url: url.to_string(),
            }
            .into());
        }
        tokio::time::sleep(backoff_delay(attempt)).await;
        attempt += 1;
    }
}

pub async fn get_profile(http: &reqwest::Client, token: &str) -> anyhow::Result<Profile> {
    get_json(http, token, reqwest::Url::parse(&format!("{API_BASE}/profile"))?).await
}

/// 백필 한 페이지. `q`가 흡수 범위를 결정한다 (DR-12).
pub async fn list_messages(
    http: &reqwest::Client,
    token: &str,
    query: &str,
    page_token: Option<&str>,
) -> anyhow::Result<MessageList> {
    let mut params: Vec<(&str, String)> = vec![
        ("q", query.to_string()),
        ("maxResults", PAGE_SIZE.to_string()),
    ];
    if let Some(page) = page_token {
        params.push(("pageToken", page.to_string()));
    }
    let url = reqwest::Url::parse_with_params(&format!("{API_BASE}/messages"), &params)?;
    get_json(http, token, url).await
}

pub async fn get_message(
    http: &reqwest::Client,
    token: &str,
    id: &str,
) -> anyhow::Result<Message> {
    let url = reqwest::Url::parse_with_params(
        &format!("{API_BASE}/messages/{id}"),
        &[("format", "full")],
    )?;
    get_json(http, token, url).await
}

/// 증분. **`None`은 "historyId가 너무 오래됐다"는 뜻이다.**
///
/// Google은 이 경우 404를 주는데, 이것을 일반 실패로 다루면 동기화가 영구 실패로
/// 굳는다. 호출자가 백필로 되돌리는 신호로 쓸 수 있도록 타입에 새겨 둔다 —
/// 에러 문자열을 매칭하는 방식은 메시지가 바뀌는 순간 조용히 깨진다.
pub async fn list_history(
    http: &reqwest::Client,
    token: &str,
    start_history_id: &str,
    page_token: Option<&str>,
) -> anyhow::Result<Option<HistoryList>> {
    // `historyTypes`는 반복 파라미터다. 삭제를 빼면 원본에서 지운 메일이 그래프에
    // 영원히 남아 "지웠는데 아직 검색된다"가 된다.
    let mut params: Vec<(&str, String)> = vec![
        ("startHistoryId", start_history_id.to_string()),
        ("historyTypes", "messageAdded".to_string()),
        ("historyTypes", "messageDeleted".to_string()),
    ];
    if let Some(page) = page_token {
        params.push(("pageToken", page.to_string()));
    }
    let url = reqwest::Url::parse_with_params(&format!("{API_BASE}/history"), &params)?;
    match get_json::<HistoryList>(http, token, url).await {
        Ok(list) => Ok(Some(list)),
        Err(error) if is_not_found(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// `get_json`이 실패에 상태 코드를 심어 두므로 그것으로 판별한다.
fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<ApiError>()
        .is_some_and(|e| e.status == 404)
}

#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub body: String,
    pub url: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Gmail API 실패 HTTP {} ({}): {}",
            self.status, self.url, self.body
        )
    }
}

impl std::error::Error for ApiError {}
