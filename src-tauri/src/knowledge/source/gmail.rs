//! Gmail 커넥터 — 설계 0020 Phase 4 / 플랜 0028.
//!
//! **읽기 전용이다.** scope는 `gmail.readonly` 하나뿐이고, 메일을 수정·삭제·발송하는
//! 코드를 여기 두지 않는다.
//!
//! OAuth client는 **사용자가 소유한다**(DR-10). Praxis가 client_id/secret을 번들하면
//! 전 사용자가 한 Cloud 프로젝트를 공유하게 되어, `gmail.readonly`가 restricted scope인
//! 탓에 CASA 보안 평가(매년 갱신)가 걸린다. 사용자마다 자기 프로젝트면 개인 사용 면제가
//! 영구히 유지된다.

/// `knowledge_sources.id` — 소스 구분 키.
pub const SOURCE_ID: &str = "gmail";

/// 키체인 account 이름. **DB가 아니라 여기에 둔다**(DR-5) — DB 파일은 백업·동기화
/// 폴더로 복사되기 쉬워서, 토큰이 섞이면 파일 한 번 유출이 계정 탈취가 된다.
///
/// client_secret은 desktop client에선 엄밀히 기밀이 아니지만(RFC 8252 public client)
/// 그래도 같이 둔다 — 예외를 두면 "어느 것이 어디 있는지"를 매번 따져야 한다.
pub const CLIENT_SECRET_KEY: &str = "knowledge_gmail_client_secret";
pub const REFRESH_TOKEN_KEY: &str = "knowledge_gmail_refresh_token";

use crate::knowledge::graph::Document;
use crate::knowledge::normalize;
use crate::knowledge::source::gmail_api::{self, Message};
use crate::knowledge::sync::{BoxChanges, Source, SourceChanges};

/// 동기화 진행 상태. `knowledge_sources.cursor` 한 칸에 인코딩한다 (플랜 0028 DR-B).
///
/// 스키마를 늘리지 않고 상태 기계를 표현할 수 있고, 무엇보다 **커서를 읽는 것만으로
/// 지금 어느 단계인지 사람이 안다** — 백필이 멈춘 건지 끝난 건지가 DB를 열면 보인다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cursor {
    /// 아직 아무것도 하지 않았다. 다음 동기화가 백필을 연다.
    Fresh,
    /// 백필 중. `history_id`는 **백필을 시작한 시점**에 확보한 값이다.
    ///
    /// 이 값을 여기 실어 나르지 않고 백필이 끝난 뒤에 조회하면, 백필이 도는 동안
    /// 도착한 메일이 증분 구간에서 통째로 빠진다 — 아무 에러 없이.
    Backfill {
        history_id: String,
        page_token: Option<String>,
    },
    /// 백필이 끝나 증분만 받는 상태.
    History { history_id: String },
}

impl Cursor {
    pub fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::Fresh;
        };
        if let Some(id) = raw.strip_prefix("history:") {
            return Self::History {
                history_id: id.to_string(),
            };
        }
        if let Some(rest) = raw.strip_prefix("backfill:") {
            // pageToken에 ':'가 들어와도 뒤쪽을 통째로 남기려면 한 번만 쪼갠다.
            let (history_id, page_token) = match rest.split_once(':') {
                Some((id, token)) => (id, token),
                None => (rest, ""),
            };
            return Self::Backfill {
                history_id: history_id.to_string(),
                page_token: (!page_token.is_empty()).then(|| page_token.to_string()),
            };
        }
        // 알 수 없는 형태는 처음부터 다시 한다. 재처리는 `content_hash` 스킵이
        // 흡수하므로 거의 공짜지만, 잘못 해석해 구간을 건너뛰면 누락은 영구적이다.
        Self::Fresh
    }

    pub fn serialize(&self) -> Option<String> {
        match self {
            Self::Fresh => None,
            Self::Backfill {
                history_id,
                page_token,
            } => Some(format!(
                "backfill:{history_id}:{}",
                page_token.as_deref().unwrap_or("")
            )),
            Self::History { history_id } => Some(format!("history:{history_id}")),
        }
    }
}

/// 메시지 하나를 그래프 문서로 만든다.
pub fn to_document(message: &Message) -> Document {
    let payload = message.payload.clone().unwrap_or_default();
    let title = payload
        .header("Subject")
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "(제목 없음)".to_string());

    Document {
        source: SOURCE_ID.to_string(),
        external_id: message.id.clone(),
        kind: "email".to_string(),
        title,
        // Gmail 웹 UI의 메시지 링크. `#all/`이라야 보관처리된 메일도 열린다.
        url: Some(format!(
            "https://mail.google.com/mail/u/0/#all/{}",
            message.id
        )),
        body: normalize::message_body(&payload),
        updated_at: internal_date_secs(&message.internal_date),
        embed: true,
    }
}

/// `internalDate`는 **밀리초** epoch 문자열이다. 그대로 쓰면 시각이 1000배로 어긋나
/// 정렬과 "최근" 판정이 전부 무너진다.
fn internal_date_secs(raw: &str) -> i64 {
    raw.parse::<i64>().unwrap_or(0) / 1000
}

/// Gmail 커넥터.
///
/// `access_token`은 호출 직전에 갱신해 주입한다 — 커넥터가 토큰 수명까지 들고 있으면
/// 인증 관심사가 동기화 로직에 스며든다.
pub struct GmailSource {
    pub http: reqwest::Client,
    pub access_token: String,
    pub query: String,
}

impl Source for GmailSource {
    fn id(&self) -> &str {
        SOURCE_ID
    }

    fn changes<'a>(&'a self, cursor: Option<&'a str>) -> BoxChanges<'a> {
        Box::pin(async move { self.fetch(Cursor::parse(cursor)).await })
    }
}

impl GmailSource {
    async fn fetch(&self, cursor: Cursor) -> anyhow::Result<SourceChanges> {
        match cursor {
            Cursor::Fresh => {
                // **먼저** historyId를 확보한다. 백필이 도는 동안 도착한 메일은
                // 이 지점 이후의 history에 남아 증분이 주워 간다.
                let profile = gmail_api::get_profile(&self.http, &self.access_token).await?;
                self.backfill_page(profile.history_id, None).await
            }
            Cursor::Backfill {
                history_id,
                page_token,
            } => self.backfill_page(history_id, page_token).await,
            Cursor::History { history_id } => self.incremental(history_id).await,
        }
    }

    /// 백필 한 페이지 (DR-C). 한 번의 `changes()`가 한 페이지만 처리하므로
    /// `sync_source`의 "커밋 후 커서 전진"이 그대로 재개 가능성이 된다.
    async fn backfill_page(
        &self,
        history_id: String,
        page_token: Option<String>,
    ) -> anyhow::Result<SourceChanges> {
        let list = gmail_api::list_messages(
            &self.http,
            &self.access_token,
            &self.query,
            page_token.as_deref(),
        )
        .await?;

        let mut upserts = Vec::with_capacity(list.messages.len());
        for reference in &list.messages {
            let message =
                gmail_api::get_message(&self.http, &self.access_token, &reference.id).await?;
            upserts.push(to_document(&message));
        }

        let has_more = list.next_page_token.is_some();
        let next = match list.next_page_token {
            Some(token) => Cursor::Backfill {
                history_id,
                page_token: Some(token),
            },
            // 마지막 페이지 — 여기서 증분으로 넘어간다.
            None => Cursor::History { history_id },
        };

        Ok(SourceChanges {
            upserts,
            deletions: Vec::new(),
            next_cursor: next.serialize(),
            // 페이지 하나는 소스의 전량이 아니다. true면 나머지가 전부 삭제로 잡힌다.
            full_scan: false,
            has_more,
        })
    }

    /// 증분은 **한 번의 호출에서 끝까지 모은다.** 백필과 달리 양이 작고, 여기서
    /// 페이지를 쪼개면 커서에 history 전용 pageToken까지 실어야 해 상태가 하나 더 는다.
    async fn incremental(&self, history_id: String) -> anyhow::Result<SourceChanges> {
        let mut added: Vec<String> = Vec::new();
        let mut deletions: Vec<String> = Vec::new();
        let mut latest = history_id.clone();
        let mut page_token: Option<String> = None;

        loop {
            let Some(list) = gmail_api::list_history(
                &self.http,
                &self.access_token,
                &history_id,
                page_token.as_deref(),
            )
            .await?
            else {
                // historyId가 만료됐다. 백필로 되돌린다 — 이 경로가 없으면 동기화가
                // 영구 실패로 굳는다.
                let profile = gmail_api::get_profile(&self.http, &self.access_token).await?;
                return self.backfill_page(profile.history_id, None).await;
            };

            for record in &list.history {
                for entry in &record.messages_added {
                    added.push(entry.message.id.clone());
                }
                for entry in &record.messages_deleted {
                    deletions.push(entry.message.id.clone());
                }
            }
            if !list.history_id.is_empty() {
                latest = list.history_id.clone();
            }
            match list.next_page_token {
                Some(token) => page_token = Some(token),
                None => break,
            }
        }

        // 같은 창에서 추가됐다 지워진 메일은 가져올 필요가 없다.
        added.retain(|id| !deletions.contains(id));
        added.dedup();

        let mut upserts = Vec::with_capacity(added.len());
        for id in &added {
            let message = gmail_api::get_message(&self.http, &self.access_token, id).await?;
            upserts.push(to_document(&message));
        }

        Ok(SourceChanges {
            upserts,
            deletions,
            next_cursor: Cursor::History { history_id: latest }.serialize(),
            full_scan: false,
            // 증분은 여기서 끝난다. true로 두면 반복 루프가 영원히 돈다 —
            // 커서(historyId)는 매번 남으므로 커서로는 종료를 판정할 수 없다.
            has_more: false,
        })
    }
}
