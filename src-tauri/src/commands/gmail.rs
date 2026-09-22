//! Gmail 지식 소스 연동 커맨드.
//!
//! `commands.rs`에서 갈라져 나왔다 — 모든 기능이 한 파일 끝에 줄을 붙이는
//! 구조가 병행 worktree의 머지 충돌을 만들었다.


use tauri::State;

use crate::knowledge;

use super::{AppState, GmailStatus, KnowledgeGmailSyncResult, gmail_access_token, now, pool_of};

#[tauri::command]
pub async fn knowledge_gmail_status(state: State<'_, AppState>) -> Result<GmailStatus, String> {
    let pool = pool_of(&state)?;
    let cfg = knowledge::config::load_gmail(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let refresh = crate::secret::get_secret(knowledge::source::gmail::REFRESH_TOKEN_KEY).await?;
    let secret = crate::secret::get_secret(knowledge::source::gmail::CLIENT_SECRET_KEY).await?;

    let cursor = knowledge::sync::load_cursor(&pool, knowledge::source::gmail::SOURCE_ID)
        .await
        .map_err(|e| e.to_string())?;
    let stage = match knowledge::source::gmail::Cursor::parse(cursor.as_deref()) {
        _ if refresh.is_none() => "미연결",
        knowledge::source::gmail::Cursor::Fresh => "백필 대기",
        knowledge::source::gmail::Cursor::Backfill { .. } => "백필 중",
        knowledge::source::gmail::Cursor::History { .. } => "최신",
    };

    let (indexed,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM knowledge_nodes WHERE source = 'gmail'")
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?;
    let last_error: Option<(Option<String>,)> =
        sqlx::query_as("SELECT last_error FROM knowledge_sources WHERE id = 'gmail'")
            .fetch_optional(&pool)
            .await
            .map_err(|e| e.to_string())?;

    // 지금 연결에 실제로 쓰일 credential이 어느 쪽인지 화면에 알려준다 — 입력란을
    // 접을지 펼칠지가 여기서 갈린다.
    let resolved = knowledge::source::gmail_auth::resolve(&cfg.client_id, secret.as_deref());
    let using_bundled = matches!(
        &resolved,
        Ok(credentials)
            if credentials.origin == knowledge::source::gmail_auth::CredentialOrigin::Bundled
    );

    Ok(GmailStatus {
        connected: refresh.is_some(),
        client_id: cfg.client_id,
        client_secret_set: secret.is_some(),
        query: cfg.query,
        stage: stage.to_string(),
        indexed,
        last_error: last_error.and_then(|r| r.0),
        bundled_available: knowledge::source::gmail_auth::bundled().is_some(),
        using_bundled,
    })
}

/// client_id·필터는 DB에, client_secret은 키체인에 나눠 저장한다 (DR-5).
#[tauri::command]
pub async fn knowledge_gmail_config_set(
    state: State<'_, AppState>,
    client_id: String,
    query: String,
    client_secret: Option<String>,
) -> Result<(), String> {
    let pool = pool_of(&state)?;
    let client_id = client_id.trim().to_string();

    // client가 바뀌면 옛 refresh token은 죽은 값이다. 다른 client로 발급된 토큰은
    // `invalid_grant`로 거부되는데, 화면에는 "재인증이 필요합니다"로만 보여
    // 연결을 지웠다 다시 만들기 전까지 원인에 도달할 수 없다. 번들↔개인 전환이
    // 생기면서 이 경로를 실제로 밟게 됐다 (ADR 0147).
    let previous = knowledge::config::load_gmail(&pool)
        .await
        .map_err(|e| e.to_string())?;
    if previous.client_id != client_id {
        crate::secret::clear_secret(knowledge::source::gmail::REFRESH_TOKEN_KEY).await?;
    }

    knowledge::config::save_gmail(
        &pool,
        &knowledge::config::GmailConfig {
            query,
            client_id: client_id.clone(),
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    // 빈 문자열은 "안 바꿈"이다. 화면이 secret을 되읽을 수 없으므로, 저장할 때마다
    // 다시 입력하게 하면 필터만 고치려던 사용자가 연결을 잃는다.
    if let Some(secret) = client_secret.filter(|s| !s.trim().is_empty()) {
        crate::secret::set_secret(knowledge::source::gmail::CLIENT_SECRET_KEY, secret.trim())
            .await?;
    }
    Ok(())
}

/// OAuth 동의 화면을 열고 콜백을 기다린다 (DR-10: loopback + PKCE).
#[tauri::command]
pub async fn knowledge_gmail_connect(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    use knowledge::source::gmail_auth as auth;
    use tauri_plugin_opener::OpenerExt;

    let pool = pool_of(&state)?;
    let cfg = knowledge::config::load_gmail(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let user_secret =
        crate::secret::get_secret(knowledge::source::gmail::CLIENT_SECRET_KEY).await?;
    let credentials =
        auth::resolve(&cfg.client_id, user_secret.as_deref()).map_err(|e| e.to_string())?;

    let (listener, redirect_uri) = auth::bind_loopback().await.map_err(|e| e.to_string())?;
    let (verifier, challenge) = auth::pkce_pair().map_err(|e| e.to_string())?;
    let csrf = auth::random_state().map_err(|e| e.to_string())?;
    let url = auth::authorize_url(&credentials.client_id, &redirect_uri, &challenge, &csrf)
        .map_err(|e| e.to_string())?;

    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("브라우저를 열지 못했습니다: {e}"))?;

    // 사용자가 계정을 고르고 동의하는 데 걸리는 시간. 짧으면 정상 사용자가 잘린다.
    let code = auth::wait_for_code(listener, &csrf, std::time::Duration::from_secs(300))
        .await
        .map_err(|e| e.to_string())?;

    let tokens = auth::exchange_code(
        &state.http,
        &credentials.client_id,
        &credentials.client_secret,
        &code,
        &verifier,
        &redirect_uri,
    )
    .await
    .map_err(|e| e.to_string())?;

    let refresh = tokens.refresh_token.ok_or(
        "refresh token이 오지 않았습니다. OAuth 동의 화면의 publishing status가 'In production'인지 확인하세요.",
    )?;
    crate::secret::set_secret(knowledge::source::gmail::REFRESH_TOKEN_KEY, &refresh).await?;

    let profile = knowledge::source::gmail_api::get_profile(&state.http, &tokens.access_token)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO knowledge_sources (id, status) VALUES ('gmail', 'connected') \
         ON CONFLICT(id) DO UPDATE SET status = 'connected', last_error = NULL",
    )
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(profile.email_address)
}

/// **파괴적이다.** 흡수한 메일 노드를 전부 지우고 자격 증명을 폐기한다.
/// 지워질 개수는 화면이 `knowledge_gmail_status.indexed`로 먼저 보여준다.
#[tauri::command]
pub async fn knowledge_gmail_disconnect(state: State<'_, AppState>) -> Result<u64, String> {
    let pool = pool_of(&state)?;
    // 키체인부터 지운다. 노드만 지우고 토큰이 남으면 "해제했는데 다시 동기화되는" 상태가 된다.
    crate::secret::clear_secret(knowledge::source::gmail::REFRESH_TOKEN_KEY).await?;
    let removed = sqlx::query("DELETE FROM knowledge_nodes WHERE source = 'gmail'")
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    // 커서를 비워 다음 연결이 백필부터 시작하게 한다.
    sqlx::query(
        "UPDATE knowledge_sources SET status = 'disconnected', cursor = NULL WHERE id = 'gmail'",
    )
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(removed)
}

/// 백필 전에 규모를 보여준다 (DR-12: 확인 후 시작).
#[tauri::command]
pub async fn knowledge_gmail_estimate(state: State<'_, AppState>) -> Result<i64, String> {
    let pool = pool_of(&state)?;
    let cfg = knowledge::config::load_gmail(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let token = gmail_access_token(&state, &pool).await?;
    let list = knowledge::source::gmail_api::list_messages(&state.http, &token, &cfg.query, None)
        .await
        .map_err(|e| e.to_string())?;
    Ok(list.result_size_estimate)
}

/// 한 번의 호출이 최대 `max_batches` 페이지를 처리한다. 남았으면 `has_more`가 참으로
/// 돌아오므로 화면이 진행률을 갱신하며 다시 부른다.
#[tauri::command]
pub async fn knowledge_gmail_sync(
    state: State<'_, AppState>,
    max_batches: Option<usize>,
) -> Result<KnowledgeGmailSyncResult, String> {
    let pool = pool_of(&state)?;
    let cfg = knowledge::config::load_gmail(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let token = gmail_access_token(&state, &pool).await?;

    let source = knowledge::source::gmail::GmailSource {
        http: state.http.clone(),
        access_token: token,
        query: cfg.query,
    };
    let report = match knowledge::sync::sync_until_done(
        &pool,
        &source,
        now(),
        max_batches.unwrap_or(5).clamp(1, 50),
    )
    .await
    {
        Ok(report) => report,
        Err(error) => {
            // 실패를 소스에 남긴다. 남기지 않으면 화면이 "연결됨"만 보여주고
            // 사용자는 왜 아무것도 안 늘어나는지 알 수 없다.
            let message = error.to_string();
            let _ = sqlx::query("UPDATE knowledge_sources SET last_error = ? WHERE id = 'gmail'")
                .bind(&message)
                .execute(&pool)
                .await;
            return Err(message);
        }
    };

    let mut embedded = 0usize;
    loop {
        let n = knowledge::graph::embed_pending(&pool, 256)
            .await
            .map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        embedded += n;
    }

    Ok(KnowledgeGmailSyncResult {
        indexed: report.indexed,
        skipped: report.skipped,
        deleted: report.deleted,
        embedded,
        has_more: report.has_more,
    })
}

