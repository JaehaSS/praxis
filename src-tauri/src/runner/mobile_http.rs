//! 모바일 PWA 정적 서빙 — `/m/*` (설계 0013 §5.2)
//!
//! 이 라우터는 `require_auth` **바깥**에 조립된다. 브라우저의 top-level navigation은
//! `Authorization` 헤더를 실을 수 없어, 앱 셸을 인증 뒤에 두면 폰에서 아예 열 수 없다.
//! 셸(HTML/JS/CSS/manifest/sw)에는 비밀이 없고 실제 인가는 전부 `/v1/*`가 수행한다.
//!
//! 번들은 `dist-mobile/`(= `npm run build:mobile` 산출물)을 바이너리에 내장한다.
//! Runner를 빌드하는 리눅스 호스트에 Node가 없을 수 있으므로 **번들이 비어 있어도
//! 컴파일과 기동은 되어야 하고**, 그 경우 503으로 원인을 분명히 알린다.

use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::Deserialize;

use crate::runner::http::RunnerHttpState;
use crate::runner::session;

#[derive(RustEmbed)]
#[folder = "../dist-mobile"]
struct MobileAssets;

/// 빌드 산출물의 진입 문서. rollup이 입력 파일명을 그대로 쓰므로 `index.html`이 아니다.
const ENTRY: &str = "mobile.html";

/// 해시가 붙는 자산 디렉터리. 이 아래만 장기 캐시한다.
const HASHED_PREFIX: &str = "assets/";

pub fn routes() -> Router<RunnerHttpState> {
    Router::new()
        .route("/m", get(entry))
        .route("/m/", get(entry))
        // 페어링 교환은 인증 밖에 있어야 한다 — 자격을 아직 갖지 못한 기기가 부르는
        // 유일한 엔드포인트다. 게이트는 일회용 코드 자체가 맡는다.
        .route("/m/pair", post(pair))
        .route("/m/*path", get(asset))
}

#[derive(Deserialize)]
struct PairRequest {
    code: String,
    #[serde(default)]
    label: String,
}

/// 일회용 코드를 세션 쿠키로 교환한다. 토큰 원문은 응답 본문에 넣지 않는다 —
/// HttpOnly 쿠키로만 전달해 XSS로도 꺼낼 수 없게 한다. (설계 0013 §6.1)
async fn pair(State(state): State<RunnerHttpState>, Json(request): Json<PairRequest>) -> Response {
    let now = crate::runner::now_secs();
    match session::redeem_pairing(&state.pool, &request.code, &request.label, now).await {
        Ok(Some(token)) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            // Secure: HTTPS(tailscale serve) 전용. SameSite=Strict: CSRF 1차 방어.
            // Max-Age는 세션 수명과 맞추고, 갱신은 서버가 슬라이딩으로 처리한다.
            let cookie = format!(
                "{}={token}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={}",
                session::COOKIE_NAME,
                session::SESSION_TTL_SECS
            );
            match HeaderValue::from_str(&cookie) {
                Ok(value) => {
                    response.headers_mut().insert(header::SET_COOKIE, value);
                    response
                }
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
        // 만료·재사용·오타를 구분해 알리지 않는다 — 코드 추측에 단서를 주지 않는다.
        Ok(None) => (
            StatusCode::UNAUTHORIZED,
            "페어링 코드가 유효하지 않거나 만료되었습니다",
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn entry() -> Response {
    serve_entry()
}

async fn asset(Path(path): Path<String>) -> Response {
    // rust-embed는 키 정확일치라 traversal이 원천 불가하지만, 의도를 코드로 남긴다.
    if path.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match MobileAssets::get(&path) {
        Some(file) => respond(&path, file.data.into_owned(), file.metadata.mimetype()),
        // 파일처럼 보이는 경로(마지막 세그먼트에 확장자)는 404. 그 외는 SPA fallback —
        // /m/t/12 같은 딥링크를 새로고침해도 셸이 떠야 한다.
        None if looks_like_file(&path) => StatusCode::NOT_FOUND.into_response(),
        None => serve_entry(),
    }
}

fn looks_like_file(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'))
}

fn serve_entry() -> Response {
    match MobileAssets::get(ENTRY) {
        Some(file) => respond(ENTRY, file.data.into_owned(), file.metadata.mimetype()),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "모바일 번들이 빌드되지 않았습니다. `npm run build:mobile` 산출물(dist-mobile/)을 \
             포함해 Runner를 다시 빌드하세요.",
        )
            .into_response(),
    }
}

fn respond(path: &str, body: Vec<u8>, mimetype: &str) -> Response {
    let cache = if path.starts_with(HASHED_PREFIX) {
        // 내용 해시가 파일명에 있으므로 영구 캐시해도 안전하다.
        "public, max-age=31536000, immutable"
    } else {
        // 셸·sw.js·manifest를 캐시하면 배포가 폰에 영영 안 닿는다.
        "no-cache"
    };
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(mimetype) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
