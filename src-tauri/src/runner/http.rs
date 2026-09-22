use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::Extension;
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db::{self, RunnerEvent};
use crate::runner::auth::RunnerAuth;
use crate::runner::config::RunnerConfig;
use crate::runner::events::{EventHub, ReplaySubscription, REPLAY_LIMIT};
use crate::runner::queue::QueueWorker;

#[derive(Clone)]
pub struct RunnerHttpState {
    pub auth: RunnerAuth,
    pub pool: SqlitePool,
    pub config: RunnerConfig,
    pub recovered_tasks: u64,
    pub events: EventHub,
    pub queue: QueueWorker,
    /// Runner 기동 시각(epoch초). uptime 표시와 "언제 되살아났는지" 판독에 쓴다.
    pub started_at: i64,
    pub review_claims: crate::review_ops::ReviewClaims,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    bind: String,
    max_concurrent_tasks: usize,
    recovered_tasks: u64,
    retention_days: i64,
    execution_policy: &'static str,
    /// 아래 4개는 모바일 상태 배너용 — "살아 있나"를 넘어 "언제부터, 무엇을 하고 있나"까지
    /// 한 번의 조회로 판독하기 위한 값이다. (설계 0013 §7.2)
    version: &'static str,
    started_at: i64,
    uptime_secs: i64,
    /// 마지막 Runner event 시각. 없으면 null — 기동 후 아무 일도 없었다는 뜻.
    last_event_at: Option<i64>,
    queued_tasks: i64,
    running_tasks: i64,
}

#[derive(Deserialize)]
struct AfterQuery {
    after: Option<i64>,
}

#[derive(Deserialize)]
struct RepositoryQuery {
    repository: String,
}

#[derive(Deserialize)]
struct QuickOpenQuery {
    #[serde(default)]
    query: String,
    /// 콤마 구분 스코프 목록(예: `task,session`). 비어 있으면 전체 허용.
    #[serde(default)]
    scopes: String,
}

#[derive(Deserialize)]
struct FileQuery {
    repository: String,
    path: String,
}

#[derive(Deserialize)]
struct BrowseQuery {
    /// 나열할 절대 경로. 비우면 첫 repository root를 연다.
    #[serde(default)]
    path: String,
}

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

#[derive(Deserialize)]
struct PathBody {
    path: String,
}

/// 경로의 git 상태 — 작업 대상이 격리(워크트리) 가능한지 프런트가 판단하는 근거.
#[derive(Serialize)]
struct GitStatusResponse {
    is_repo: bool,
}

/// 디렉터리 한 단계 + 상위 경로 — 브라우저가 위로 올라갈 수 있게 함께 준다.
#[derive(Serialize)]
struct BrowseResponse {
    path: String,
    /// 허용된 root를 벗어나면 `None` — 브라우저는 "위로" 버튼을 숨긴다.
    parent: Option<String>,
    entries: Vec<crate::fsapi::DirEntryInfo>,
}

#[derive(Deserialize)]
struct GithubIssuesQuery {
    repository: String,
}

#[derive(Deserialize)]
struct GithubIssueDeleteQuery {
    repository: String,
    number: u64,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum GithubIssuesResponse {
    Ready {
        owner_repo: String,
        issues: Vec<crate::github::GhIssue>,
    },
    Unavailable,
    NotGithubRepo,
}

#[derive(Deserialize)]
struct GithubReposRequest {
    repositories: Vec<String>,
}

#[derive(Deserialize)]
struct GithubIssueTaskCreateRequest {
    repository: String,
    number: u64,
    agent: String,
}

#[derive(Deserialize)]
struct FileWriteRequest {
    repository: String,
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct TaskInputRequest {
    data: String,
}

#[derive(Deserialize)]
struct TaskMessageRequest {
    message: String,
}

/// 세션 모델 오버라이드 교체 본문. 빈 문자열은 해제(벤더 기본 복귀)라 유효한 값이다 —
/// `Option`이 아닌 `String`인 이유이고, 그래서 필드 누락과 해제를 서로 구분한다.
#[derive(Deserialize)]
struct TaskModelRequest {
    model: String,
}
#[derive(Deserialize)]
struct ReceiptSubmitRequest {
    message: String,
    #[serde(default)]
    image_paths: Vec<String>,
}

#[derive(Deserialize)]
struct SideQuestionCancelRequest {
    turn_id: i64,
}

#[derive(Deserialize)]
struct SideQuestionResetRequest {
    generation: i64,
}

#[derive(Deserialize)]
struct ScheduleCreateRequest {
    label: String,
    cron: String,
    kind: String,
    payload: String,
    tz_offset_secs: i32,
}

#[derive(Deserialize)]
struct ScheduleEnabledRequest {
    enabled: bool,
}

#[derive(Deserialize)]
struct ReminderCreateRequest {
    text: String,
    delay_minutes: i64,
}

#[derive(Serialize)]
struct Watermark {
    kind: &'static str,
    sequence: i64,
}

#[derive(Serialize)]
pub struct PushKeyResponse {
    pub public_key: String,
}

#[derive(Deserialize)]
struct PushSubscribeRequest {
    endpoint: String,
}

#[derive(Deserialize)]
struct PushUnsubscribeRequest {
    endpoint: String,
}

/// 일회용 페어링 코드. 원문은 QR 표시 용도로 이 응답에만 존재하고 DB에는 해시만 남는다.
#[derive(Serialize)]
pub struct PairingResponse {
    pub code: String,
    pub expires_at: i64,
}

/// 모바일 PWA와 데스크톱 원격 클라이언트가 공유하는 라우트 — 두 호스트(Runner·데스크톱)
/// 모두에서 마운트된다. 작업 행위는 `TaskActions` Extension이 호스트별로 수행한다.
/// (설계 2026-09-13 D1·D4)
pub fn mobile_surface_routes() -> Router<RunnerHttpState> {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/tasks", get(tasks).post(task_create))
        .route("/v1/notifications/results", get(notification_results))
        .route("/v1/tasks/:id", get(task))
        .route("/v1/tasks/:id/run", post(task_run_approve))
        .route("/v1/tasks/:id/approve", post(task_approve))
        .route("/v1/tasks/:id/approval-status", get(task_approval_status))
        .route("/v1/tasks/:id/approval-repair", get(approval_repair_status).post(approval_repair_prepare))
        .route("/v1/tasks/:id/approval-repair/run", post(approval_repair_run))
        .route("/v1/tasks/:id/approval-repair/cancel", post(approval_repair_cancel))
        .route("/v1/tasks/:id/approval-repair/accept", post(approval_repair_accept))
        .route("/v1/tasks/:id/discard", post(task_discard))
        .route("/v1/tasks/:id/diff", get(task_diff))
        .route("/v1/tasks/:id/diff/hunks", get(task_diff_hunks))
        .route("/v1/tasks/:id/output", get(task_output))
        .route("/v1/tasks/:id/message", post(task_message))
        .route("/v1/repositories", get(repositories))
        .route("/v1/files/tree", get(file_tree))
        .route("/v1/files/read", get(file_read))
        .route("/v1/schedules", get(schedules).post(schedule_create))
        .route(
            "/v1/schedules/:id",
            delete(schedule_delete).patch(schedule_set_enabled),
        )
        .route("/v1/reminders", post(reminder_create))
        .route("/v1/events", get(events))
        .route("/v1/events/live", get(live_events))
        .route("/v1/push/key", get(push_key))
        .route(
            "/v1/push/subscribe",
            post(push_subscribe).delete(push_unsubscribe),
        )
        .route("/v1/mobile/pairings", post(mobile_pairing_create))
        .route("/v1/mobile/sessions", get(mobile_sessions))
        .route("/v1/mobile/sessions/:id", delete(mobile_session_revoke))
        .merge(crate::runner::review_http::routes())
}

/// 인증·CORS·`/m/*` 셸까지 얹은 완성 라우터. 호스트가 `actions`로 작업 행위 구현을 준다.
/// `router()`(Runner 전체)와 데스크톱 모바일 표면이 같은 조립을 쓴다.
pub fn finish_router(
    routes: Router<RunnerHttpState>,
    state: RunnerHttpState,
    actions: crate::runner::actions::SharedTaskActions,
) -> Router {
    routes
        .layer(Extension(actions))
        .layer(axum::middleware::from_fn_with_state(
            crate::runner::auth::AuthState {
                auth: state.auth.clone(),
                pool: state.pool.clone(),
            },
            crate::runner::auth::require_auth,
        ))
        // 인증 레이어 바깥(outer) — 브라우저 preflight(OPTIONS)는 Authorization을 싣지
        // 않으므로 인증 앞에서 응답해야 한다. 실제 요청 인가는 loopback+token이 담당
        .layer(axum::middleware::from_fn(cors))
        // 모바일 PWA 셸(/m/*)은 layer 이후에 merge해 **인증 바깥**에 둔다. 브라우저의
        // top-level navigation은 Authorization을 실을 수 없어 셸을 인증 뒤에 두면 폰에서
        // 열리지 않는다. 셸에 비밀은 없고, 인가는 위의 /v1/* 라우트가 담당한다. (설계 0013 §5.2)
        .merge(crate::runner::mobile_http::routes())
        .with_state(state)
}

/// 데스크톱이 띄우는 모바일 표면 — 공유 라우트만, 작업 행위는 데스크톱 구현.
pub fn mobile_surface_router(
    state: RunnerHttpState,
    actions: crate::runner::actions::SharedTaskActions,
) -> Router {
    finish_router(mobile_surface_routes(), state, actions)
}

/// Runner 전체 API — 공유 라우트 + Runner 전용(취소·삭제·PTY 입력·파일 쓰기·GitHub 등).
pub fn router(state: RunnerHttpState) -> Router {
    router_with_workflow(state, None)
}

pub fn router_with_workflow(
    state: RunnerHttpState,
    workflow: Option<std::sync::Arc<crate::runner::workflow::WorkflowService>>,
) -> Router {
    let routes = mobile_surface_routes()
        .route("/v1/sessions", get(sessions))
        .route("/v1/tasks/:id", delete(task_delete))
        .route("/v1/tasks/:id/cancel", post(task_cancel))
        .route(
            "/v1/tasks/:id/annotations",
            get(task_annotations_list).post(task_annotation_save),
        )
        .route(
            "/v1/tasks/:id/annotations/resend",
            post(task_annotations_resend),
        )
        .route("/v1/tasks/:id/partial/apply", post(task_partial_apply))
        .route(
            "/v1/tasks/:id/partial/rollback",
            post(task_partial_rollback),
        )
        .route("/v1/ensembles/:ensemble/matrix", get(ensemble_matrix))
        .route("/v1/ensembles/:ensemble/compose", post(ensemble_compose))
        .route("/v1/tasks/:id/input", post(task_input))
        // `/v1/tasks/:id/message`는 공유 라우트로 옮겨 갔다 — 여기 두면 중복 등록으로 axum이 패닉한다.
        .route("/v1/tasks/:id/model", put(task_model_set))
        .route(
            "/v1/tasks/:id/message-receipts/:request_id",
            get(conversation_receipt).post(conversation_submit),
        )
        .route("/v1/tasks/:id/side-question", get(side_question_read))
        .route(
            "/v1/tasks/:id/side-question/messages",
            post(side_question_send),
        )
        .route(
            "/v1/tasks/:id/side-question/cancel",
            post(side_question_cancel),
        )
        .route(
            "/v1/tasks/:id/side-question/reset",
            post(side_question_reset),
        )
        .route("/v1/quickopen", get(quickopen))
        .route(
            "/v1/github/issues",
            get(github_issues).delete(github_issue_delete),
        )
        .route("/v1/github/repos", post(github_repos))
        .route("/v1/github/issues/task", post(github_issue_task_create))
        .route("/v1/files/browse", get(file_browse))
        .route("/v1/files/roots", get(file_roots))
        .route("/v1/git/status", get(git_status))
        .route("/v1/git/init", post(git_init))
        .route("/v1/files/write", put(file_write))
        .route("/v1/skills", get(skills_list))
        .merge(crate::runner::memory_http::routes())
        .merge(crate::runner::review_process_http::routes())
        .merge(crate::runner::workflow::http::routes(workflow));
    finish_router(
        routes,
        state,
        std::sync::Arc::new(crate::runner::actions::RunnerTaskActions),
    )
}

/// Desktop webview의 교차 출처 fetch(예: http://localhost:1420 → http://127.0.0.1:포트)를
/// 허용한다. CORS는 여기서 보안 경계가 아니다 — 어떤 로컬 프로세스든 이미 curl로 접근
/// 가능하며, 접근 통제는 require_auth의 loopback peer + pairing token이 수행한다.
async fn cors(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if request.method() == Method::OPTIONS {
        return cors_preflight();
    }
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response
}

fn cors_preflight() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, DELETE"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    response
}

async fn health(State(state): State<RunnerHttpState>) -> Json<HealthResponse> {
    // 집계 실패로 health 자체가 죽으면 "러너 다운"과 구분되지 않는다 — 개별 값은 폴백한다.
    let last_event_at = db::latest_runner_event_ts(&state.pool)
        .await
        .unwrap_or(None);
    let queued_tasks = db::count_tasks_in_state(&state.pool, db::state::QUEUED)
        .await
        .unwrap_or(-1);
    let running_tasks = db::count_tasks_in_state(&state.pool, db::state::RUNNING)
        .await
        .unwrap_or(-1);
    Json(HealthResponse {
        status: "ok",
        bind: state.config.bind.to_string(),
        max_concurrent_tasks: state.config.max_concurrent_tasks,
        recovered_tasks: state.recovered_tasks,
        retention_days: crate::runner::RETENTION_DAYS,
        execution_policy: state.config.execution_policy.as_str(),
        version: env!("CARGO_PKG_VERSION"),
        started_at: state.started_at,
        uptime_secs: (now() - state.started_at).max(0),
        last_event_at,
        queued_tasks,
        running_tasks,
    })
}

/// 브라우저 `applicationServerKey`. 키가 없으면 이 시점에 만들어 저장한다.
async fn push_key() -> Result<Json<PushKeyResponse>, (StatusCode, String)> {
    let keys =
        crate::runner::push::VapidKeys::load_or_create(&crate::runner::push::default_key_path())
            .map_err(internal_error)?;
    Ok(Json(PushKeyResponse {
        public_key: keys.public_key_base64url(),
    }))
}

async fn push_subscribe(
    State(state): State<RunnerHttpState>,
    Extension(context): Extension<crate::runner::auth::AuthContext>,
    Json(request): Json<PushSubscribeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    // 세션을 회수하면 그 기기의 구독도 함께 사라져야 한다 — 소유자를 기록해 둔다.
    let session_id = match context {
        crate::runner::auth::AuthContext::Mobile(session) => Some(session.id),
        crate::runner::auth::AuthContext::Pairing => None,
    };
    crate::runner::push::subscribe(&state.pool, &request.endpoint, session_id, now())
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn push_unsubscribe(
    State(state): State<RunnerHttpState>,
    Json(request): Json<PushUnsubscribeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    crate::runner::push::unsubscribe(&state.pool, &request.endpoint)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// QR로 표시할 일회용 페어링 코드를 발급한다. 원문은 이 응답에만 실린다. (설계 0013 §6.1)
async fn mobile_pairing_create(
    State(state): State<RunnerHttpState>,
) -> Result<Json<PairingResponse>, (StatusCode, String)> {
    let now = now();
    let (code, expires_at) = crate::runner::session::create_pairing(&state.pool, now)
        .await
        .map_err(internal_error)?;
    Ok(Json(PairingResponse { code, expires_at }))
}

/// 연결된 모바일 기기 목록. 토큰 해시는 절대 싣지 않는다.
async fn mobile_sessions(
    State(state): State<RunnerHttpState>,
) -> Result<Json<Vec<crate::runner::session::MobileSession>>, (StatusCode, String)> {
    crate::runner::session::list_sessions(&state.pool, now())
        .await
        .map(Json)
        .map_err(internal_error)
}

/// 기기 하나를 즉시 끊는다. Desktop 연결에는 영향이 없다.
async fn mobile_session_revoke(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let revoked = crate::runner::session::revoke_session(&state.pool, id, now())
        .await
        .map_err(internal_error)?;
    // 기기를 끊으면 그 기기의 푸시 구독도 함께 사라져야 한다 — 남으면 회수한 폰이
    // 계속 알림을 받는다.
    let _ = crate::runner::push::remove_for_session(&state.pool, id).await;
    if revoked {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            "이미 회수되었거나 없는 기기입니다".to_string(),
        ))
    }
}

async fn tasks(
    State(state): State<RunnerHttpState>,
) -> Result<Json<Vec<db::Task>>, (StatusCode, String)> {
    db::list_tasks(&state.pool)
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Durable task-result source. `after` omitted establishes a watermark baseline.
async fn notification_results(
    State(state): State<RunnerHttpState>,
    Query(query): Query<AfterQuery>,
) -> Result<Json<crate::notifications::SourcePage>, (StatusCode, String)> {
    crate::notifications::source_page(&state.pool, query.after)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn task(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<Json<db::Task>, (StatusCode, String)> {
    let task = db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?;
    task.map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "task를 찾을 수 없습니다".to_string()))
}

async fn task_delete(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let _review_claim = state
        .review_claims
        .claim_finalization(id)
        .map_err(invalid_request)?;
    crate::runner::review_process::assert_task_unfenced(&state.pool, id)
        .await
        .map_err(invalid_request)?;
    crate::runner::delete_finished_task(&state.pool, id)
        .await
        .map_err(invalid_request)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `resume_session`이 실린 요청은 모바일 자격에서 거절한다(설계 2026-09-17 제약 5) —
/// `mobile_scope_denies`는 `(method, path)`만 보고 본문을 못 보므로, 모바일이 정상적으로 쓰는
/// `POST /v1/tasks`를 여기서 직접 갈라야 한다.
async fn task_create(
    State(state): State<RunnerHttpState>,
    Extension(context): Extension<crate::runner::auth::AuthContext>,
    Json(request): Json<crate::runner::QueuedTaskRequest>,
) -> Result<Json<db::Task>, (StatusCode, String)> {
    if request.resume_session.is_some() && matches!(context, crate::runner::auth::AuthContext::Mobile(_))
    {
        return Err((
            StatusCode::FORBIDDEN,
            "모바일 세션은 세션을 이어받을 수 없습니다".to_string(),
        ));
    }
    crate::runner::create_queued_task(
        &state.config,
        &state.pool,
        &state.queue.worktree_locks(),
        request,
        now(),
    )
    .await
    .map(Json)
    .map_err(create_task_error_response)
}

/// [`crate::runner::CreateTaskError`] → HTTP 응답(설계 2026-09-17 결정 9). `Invalid`는 400,
/// 세션 해석·인가 실패는 404(같은 얼굴 — 존재 열거 방지), 중복 승계는 409 + 진행 중인 작업 id.
pub(crate) fn create_task_error_response(
    error: crate::runner::CreateTaskError,
) -> (StatusCode, String) {
    use crate::runner::CreateTaskError;
    let message = error.to_string();
    match error {
        CreateTaskError::Invalid(_) => (StatusCode::BAD_REQUEST, message),
        CreateTaskError::SessionUnavailable => (StatusCode::NOT_FOUND, message),
        CreateTaskError::Conflict(task_id) => (
            StatusCode::CONFLICT,
            serde_json::json!({ "error": message, "task_id": task_id }).to_string(),
        ),
    }
}

#[derive(Deserialize)]
struct SessionsQuery {
    /// 특정 저장소 루트로 좁힌다(기본). `repository_roots`에 인가되지 않으면 거절.
    #[serde(default)]
    repository: Option<String>,
    /// 켜면 `repository`를 무시하고 인가된 모든 저장소 루트를 대상으로 한다(설계 결정 10).
    #[serde(default)]
    all: bool,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

const SESSIONS_DEFAULT_LIMIT: usize = 200;
/// 호출자가 올릴 수 있는 상한. `scan`은 세션홈 전체를 순회하므로(실측 1,568파일·1.2GB) 값을
/// 그대로 믿으면 인가된 클라이언트 하나가 러너를 붙잡아 둘 수 있다. 데스크톱 커맨드는 아예
/// 상한을 파라미터로 받지 않는다 — 여기만 값을 받으므로 여기서 깎는다.
const SESSIONS_MAX_LIMIT: usize = 500;

#[derive(Serialize)]
struct SessionsResponse {
    /// `sessionhome::SessionMeta`를 그대로 싣는다 — 필드가 1:1인 사본을 여기 두면 두 벌이
    /// 어긋난다(원장 #236과 같은 종류의 값을 이미 치렀다).
    sessions: Vec<crate::sessionhome::SessionMeta>,
    /// 목록이 비었을 때만 채운다 — 어떤 기준 경로로 걸렀는지(페어링 자격 전용, 설계 결정 9).
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic: Option<SessionsDiagnostic>,
}

#[derive(Serialize)]
struct SessionsDiagnostic {
    cwd_prefixes: Vec<String>,
}

/// Runner 전용 — 세션홈을 저장소 루트로 좁혀 벤더 세션을 목록으로 낸다(설계 2026-09-17).
/// 모바일 자격은 `mobile_scope_denies`가 경로 단위로 막는다. `repository_roots` 밖 cwd의
/// 세션은 이 목록에 나타나지 않는다 — `cwd_prefixes`가 항상 인가된 루트에서만 나온다.
async fn sessions(
    State(state): State<RunnerHttpState>,
    Extension(context): Extension<crate::runner::auth::AuthContext>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<SessionsResponse>, (StatusCode, String)> {
    let cwd_prefixes = if query.all {
        state
            .config
            .repository_roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    } else if let Some(repository) = query.repository.as_deref() {
        let authorized = authorized_repository(&state, repository)?;
        vec![authorized.to_string_lossy().into_owned()]
    } else {
        state
            .config
            .repository_roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    let limit = query
        .limit
        .unwrap_or(SESSIONS_DEFAULT_LIMIT)
        .clamp(1, SESSIONS_MAX_LIMIT);
    let filter = crate::sessionhome::ScanFilter {
        cwd_prefixes: cwd_prefixes.clone(),
        query: query.query.clone(),
    };
    let sessions = tokio::task::spawn_blocking(move || crate::sessionhome::scan(&filter, limit))
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("세션홈 조회 실패: {error}"),
            )
        })?;
    // 빈 목록의 진단(기준 경로)은 페어링 자격에만 싣는다 — 모바일에 노출하면 경로 구조가 샌다.
    let diagnostic = if sessions.is_empty() && matches!(context, crate::runner::auth::AuthContext::Pairing) {
        Some(SessionsDiagnostic { cwd_prefixes })
    } else {
        None
    };
    Ok(Json(SessionsResponse {
        sessions,
        diagnostic,
    }))
}

async fn task_cancel(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let _review_claim = state
        .review_claims
        .claim_finalization(id)
        .map_err(invalid_request)?;
    crate::runner::review_process::assert_task_unfenced(&state.pool, id)
        .await
        .map_err(invalid_request)?;
    if state
        .queue
        .cancel(id, now())
        .await
        .map_err(invalid_request)?
    {
        return Ok(StatusCode::NO_CONTENT);
    }
    let cancelled = db::cancel_queued_task(&state.pool, id, now())
        .await
        .map_err(internal_error)?;
    if cancelled {
        Ok(StatusCode::NO_CONTENT)
    } else {
        crate::runner::cancel_pending_task(
            &state.config,
            &state.pool,
            &state.queue.worktree_locks(),
            id,
            now(),
        )
        .await
        .map_err(|error| (StatusCode::CONFLICT, error))?;
        Ok(StatusCode::NO_CONTENT)
    }
}

async fn task_run_approve(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<Json<db::Task>, (StatusCode, String)> {
    let _review_claim = state
        .review_claims
        .claim_finalization(id)
        .map_err(invalid_request)?;
    crate::runner::review_process::assert_task_unfenced(&state.pool, id)
        .await
        .map_err(invalid_request)?;
    crate::runner::approve_pending_task(&state.pool, &state.queue.worktree_locks(), id, now())
        .await
        .map(Json)
        .map_err(invalid_request)
}

async fn task_approval_status(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<Json<crate::approval::Status>, (StatusCode, String)> {
    let task = db::get_task(&state.pool, id).await.map_err(internal_error)?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "작업을 찾을 수 없습니다".into()))?;
    let repo = authorized_repository(&state, &task.repo)?;
    let path = authorized_repository(&state, &task.worktree_path)?;
    let worktree = crate::worktree::Worktree {
        repo, path, branch: task.branch.clone(), base: task.base.clone(), base_revision: task.base_revision.clone(),
    };
    crate::approval::inspect(&state.pool, &task, worktree).await.map(Json).map_err(internal_error)
}

#[derive(Deserialize)]
struct RepairBody { session_id: String }

async fn repair_task(state: &RunnerHttpState, id: i64) -> Result<db::Task, (StatusCode, String)> {
    let task = db::get_task(&state.pool, id).await.map_err(internal_error)?.ok_or_else(|| (StatusCode::NOT_FOUND, "작업을 찾을 수 없습니다".into()))?;
    authorized_repository(state, &task.repo)?;
    authorized_repository(state, &task.worktree_path)?;
    Ok(task)
}

async fn approval_repair_status(State(state): State<RunnerHttpState>, Path(id): Path<i64>) -> Result<Json<Option<crate::approval::repair::Session>>, (StatusCode, String)> {
    repair_task(&state, id).await?;
    crate::approval::repair::observed_status(&state.pool, id, &state.review_claims).await.map(Json).map_err(internal_error)
}

async fn approval_repair_prepare(State(state): State<RunnerHttpState>, Path(id): Path<i64>) -> Result<Json<crate::approval::repair::Session>, (StatusCode, String)> {
    let task = repair_task(&state, id).await?;
    let worktree = crate::worktree::Worktree { repo: task.repo.clone().into(), path: task.worktree_path.clone().into(), branch: task.branch.clone(), base: task.base.clone(), base_revision: task.base_revision.clone() };
    crate::approval::repair::prepare(&state.pool, &task, worktree, &state.review_claims).await.map(Json).map_err(|e| invalid_request(e.to_string()))
}

async fn approval_repair_run(State(state): State<RunnerHttpState>, Path(id): Path<i64>, Json(body): Json<RepairBody>) -> Result<Json<crate::approval::repair::Session>, (StatusCode, String)> {
    let task = repair_task(&state, id).await?;
    let model = match task.model.clone().filter(|m| !m.trim().is_empty()) {
        Some(model) => Some(model), None => db::get_setting(&state.pool, &format!("model:{}", task.agent.as_deref().unwrap_or("claude"))).await.map_err(internal_error)?,
    };
    crate::approval::repair::run(state.pool, task, state.review_claims, body.session_id, model).await.map(Json).map_err(invalid_request)
}

async fn approval_repair_accept(State(state): State<RunnerHttpState>, Path(id): Path<i64>, Json(body): Json<RepairBody>) -> Result<Json<crate::approval::repair::Session>, (StatusCode, String)> {
    let task = repair_task(&state, id).await?;
    crate::approval::repair::accept(&state.pool, &task, &state.review_claims, &body.session_id).await.map(Json).map_err(|e| invalid_request(e.to_string()))
}

async fn approval_repair_cancel(State(state): State<RunnerHttpState>, Path(id): Path<i64>, Json(body): Json<RepairBody>) -> Result<StatusCode, (StatusCode, String)> {
    repair_task(&state, id).await?;
    crate::approval::repair::cancel(&state.pool, id, &body.session_id).await.map_err(|e| invalid_request(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn task_approve(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let _review_claim = state
        .review_claims
        .claim_finalization(id)
        .map_err(invalid_request)?;
    crate::runner::review_process::assert_task_unfenced(&state.pool, id)
        .await
        .map_err(invalid_request)?;
    crate::runner::finalize_task(
        &state.config,
        &state.pool,
        &state.queue.worktree_locks(),
        id,
        true,
        now(),
    )
    .await
    .map_err(invalid_request)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn task_discard(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let _review_claim = state
        .review_claims
        .claim_finalization(id)
        .map_err(invalid_request)?;
    crate::runner::review_process::assert_task_unfenced(&state.pool, id)
        .await
        .map_err(invalid_request)?;
    crate::runner::finalize_task(
        &state.config,
        &state.pool,
        &state.queue.worktree_locks(),
        id,
        false,
        now(),
    )
    .await
    .map_err(invalid_request)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn task_output(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Query(query): Query<AfterQuery>,
) -> Result<Json<Vec<db::TaskOutput>>, (StatusCode, String)> {
    // task별 SQL 필터 — 전역 sequence로 자르면 다른 task의 출력에 밀려 빈 페이지가
    // 생겨 클라이언트가 "끝"과 "더 있음"을 구분할 수 없다. 빈 응답 = 더 없음 보장.
    let output = db::list_task_output_for_task_after(
        &state.pool,
        id,
        query.after.unwrap_or(0).max(0),
        REPLAY_LIMIT,
    )
    .await
    .map_err(internal_error)?;
    Ok(Json(output))
}

/// 실행 중 terminal task의 PTY stdin에 원시 입력을 전달한다.
///
/// 로컬 `task_write`와 동일한 신뢰 수준 — 접근 통제는 require_auth(loopback+token)가
/// 담당하며, 활성 세션이 없으면 입력 유실 대신 CONFLICT로 알린다.
async fn task_input(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<TaskInputRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    match state
        .queue
        .write_terminal_input(id, request.data.as_bytes())
    {
        None => Err((
            StatusCode::CONFLICT,
            "실행 중인 terminal 작업에만 입력을 보낼 수 있습니다".to_string(),
        )),
        Some(Err(error)) => Err(internal_error(error)),
        Some(Ok(())) => Ok(StatusCode::NO_CONTENT),
    }
}

/// 검토 대기 conversation task에 후속 턴 메시지를 보낸다.
///
/// instruction을 교체해 재큐잉하면 queue worker가 `convo_session_id` resume으로
/// 다음 턴을 실행한다. 진행 중(Running/Queued)이거나 종료된 작업은 재큐잉되지 않는다.
async fn task_message(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<TaskMessageRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message는 필수입니다".to_string()));
    }
    let task = db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or_else(not_found)?;
    if task.mode != "conversation" {
        return Err((
            StatusCode::CONFLICT,
            "conversation 작업에만 후속 메시지를 보낼 수 있습니다".to_string(),
        ));
    }
    if db::requeue_conversation_followup(&state.pool, id, message, now())
        .await
        .map_err(internal_error)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::CONFLICT,
            "검토 대기 상태의 대화 작업만 이어갈 수 있습니다".to_string(),
        ))
    }
}

/// 세션 모델 오버라이드를 교체한다 — 데스크톱 `task_model_set`의 원격 짝이다.
///
/// 본체는 같은 `set_task_model_checked`다. 검사(프리셋 에이전트만·effort 동반 정리·에이전트
/// 전환 레이스)를 여기서 다시 쓰면 두 경로가 서로 다른 규칙으로 갈라지고, 그 어긋남은 다음 턴이
/// CLI에 닿아서야 드러난다.
///
/// 진행 중인 턴은 프로세스가 이미 떠 있어 바뀌지 않는다. 다음 턴은 큐 워커가 행을 새로 읽으므로
/// (`resume_conversation` → `db::get_task`) 여기서 쓴 값이 그대로 실린다.
async fn task_model_set(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<TaskModelRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    // 없는 작업을 400으로 돌려주면 클라이언트가 "모델 이름이 틀렸다"로 읽는다. 존재 여부는
    // 공통 본체가 문자열 오류로만 알려주므로 여기서 먼저 갈라 404를 준다.
    if db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?
        .is_none()
    {
        return Err(not_found());
    }
    crate::commands::set_task_model_checked(&state.pool, id, &request.model)
        .await
        .map_err(invalid_request)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn conversation_receipt(
    State(state): State<RunnerHttpState>,
    Path((id, request_id)): Path<(i64, String)>,
) -> Result<Json<crate::side_question::ConversationReceipt>, (StatusCode, String)> {
    crate::side_question::receipt_read(&state.pool, id, &request_id)
        .await
        .map(Json)
        .map_err(invalid_request)
}

async fn conversation_submit(
    State(state): State<RunnerHttpState>,
    Path((id, request_id)): Path<(i64, String)>,
    Json(request): Json<ReceiptSubmitRequest>,
) -> Result<Json<crate::side_question::ConversationReceipt>, (StatusCode, String)> {
    let Some(task) = db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?
    else {
        return Ok(Json(crate::side_question::ConversationReceipt {
            request_id,
            status: "not_found".into(),
            error: None,
        }));
    };
    let _receipt_admission = crate::side_question::receipt_admission_lock(id, &request_id).await;
    if let Some(receipt) = crate::side_question::receipt_begin(
        &state.pool,
        id,
        &request_id,
        &request.message,
        &request.image_paths,
        now(),
    )
    .await
    .map_err(invalid_request)?
    {
        return Ok(Json(receipt));
    }
    if crate::side_question::blocks_main_execution(&state.pool, id).await.map_err(invalid_request)? {
        return crate::side_question::receipt_read(&state.pool, id, &request_id).await.map(Json).map_err(invalid_request);
    }
    if !request.image_paths.is_empty() {
        let receipt = crate::side_question::receipt_finish(
            &state.pool,
            id,
            &request_id,
            "failed",
            Some("Runner 대화 메시지는 이미지 첨부를 지원하지 않습니다"),
        )
        .await
        .map_err(invalid_request)?;
        return Ok(Json(receipt));
    }
    let outcome = if task.mode == "conversation"
        && db::requeue_conversation_followup_receipt(&state.pool, id, &request_id, request.message.trim(), now())
            .await
            .map_err(internal_error)?
    {
        crate::side_question::receipt_read(&state.pool, id, &request_id).await
    } else {
        let existing = crate::side_question::receipt_read(&state.pool, id, &request_id).await.map_err(invalid_request)?;
        if existing.status == "accepted" { return Ok(Json(existing)); }
        crate::side_question::receipt_finish(
            &state.pool,
            id,
            &request_id,
            "failed",
            Some("검토 대기 상태의 conversation 작업만 이어갈 수 있습니다"),
        )
        .await
    };
    outcome.map(Json).map_err(invalid_request)
}

async fn side_question_read(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<Json<crate::side_question::SideQuestionSnapshot>, (StatusCode, String)> {
    crate::side_question::read(&state.pool, id, now())
        .await
        .map(Json)
        .map_err(invalid_request)
}

async fn side_question_send(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(input): Json<crate::side_question::SideQuestionSend>,
) -> Result<Json<crate::side_question::SideQuestionSnapshot>, (StatusCode, String)> {
    let (turn_id, inserted) = crate::side_question::send(&state.pool, id, input, now())
        .await
        .map_err(invalid_request)?;
    if inserted {
        state.queue.spawn_side_question(id, turn_id, now());
    }
    crate::side_question::read(&state.pool, id, now())
        .await
        .map(Json)
        .map_err(invalid_request)
}

async fn side_question_cancel(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<SideQuestionCancelRequest>,
) -> Result<Json<crate::side_question::SideQuestionSnapshot>, (StatusCode, String)> {
    crate::side_question::cancel(&state.pool, id, request.turn_id, now())
        .await
        .map_err(invalid_request)?;
    crate::side_question::read(&state.pool, id, now())
        .await
        .map(Json)
        .map_err(invalid_request)
}

async fn side_question_reset(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<SideQuestionResetRequest>,
) -> Result<Json<crate::side_question::SideQuestionSnapshot>, (StatusCode, String)> {
    crate::side_question::reset(&state.pool, id, request.generation, now())
        .await
        .map_err(invalid_request)?;
    crate::side_question::read(&state.pool, id, now())
        .await
        .map(Json)
        .map_err(invalid_request)
}

/// diff 범위 쿼리(`?range=uncommitted`). 생략하면 세션 전체다 — 구버전 클라이언트가
/// 인자 없이 불러도 종전과 같은 응답을 받는다.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
struct DiffRangeQuery {
    #[serde(default)]
    range: crate::worktree::DiffRange,
}

async fn task_diff(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Query(query): Query<DiffRangeQuery>,
) -> Result<Json<crate::worktree::TaskDiffResult>, (StatusCode, String)> {
    let task = db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or_else(not_found)?;
    let root = authorized_repository(&state, &task.worktree_path)?;
    if !crate::worktree::is_git_repository(&root) {
        return Ok(Json(crate::worktree::TaskDiffResult {
            files: vec![],
            baseline: crate::worktree::BaselineStatus::Legacy,
        }));
    }
    let worktree = crate::worktree::Worktree {
        repo: task.repo.into(),
        path: root,
        branch: task.branch,
        base: task.base,
        base_revision: task.base_revision,
    };
    let files = worktree
        .diff_detailed_range(query.range)
        .map_err(internal_error)?;
    Ok(Json(crate::worktree::TaskDiffResult {
        baseline: worktree.baseline_status(),
        files,
    }))
}

/// 구조화 hunk 목록(`diffmodel`) — local(Tauri command)과 동일한 `diffmodel::build_hunks`를
/// 공유해 parity를 보장한다(B-1 주석·B-2 부분 승인·B-3 ensemble 조합의 공통 조회 API).
async fn task_diff_hunks(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Query(query): Query<DiffRangeQuery>,
) -> Result<Json<Vec<crate::diffmodel::DiffHunk>>, (StatusCode, String)> {
    let task = db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or_else(not_found)?;
    task_hunks_range(&state, &task, query.range).map(Json)
}

/// task의 현재 구조화 hunk 목록 — `task_diff_hunks`와 annotations 재매칭/재전송이 공유한다.
fn task_hunks(
    state: &RunnerHttpState,
    task: &db::Task,
) -> Result<Vec<crate::diffmodel::DiffHunk>, (StatusCode, String)> {
    task_hunks_range(state, task, crate::worktree::DiffRange::Session)
}

/// 범위를 골라 뜨는 hunk 목록 — 로컬 `task_hunks_range`와 같은 규칙을 쓴다.
fn task_hunks_range(
    state: &RunnerHttpState,
    task: &db::Task,
    range: crate::worktree::DiffRange,
) -> Result<Vec<crate::diffmodel::DiffHunk>, (StatusCode, String)> {
    let root = authorized_repository(state, &task.worktree_path)?;
    if !crate::worktree::is_git_repository(&root) {
        return Ok(vec![]);
    }
    let worktree = crate::worktree::Worktree {
        repo: task.repo.clone().into(),
        path: root,
        branch: task.branch.clone(),
        base: task.base.clone(),
        base_revision: task.base_revision.clone(),
    };
    let diff_text = worktree
        .diff_unified_range(3, range)
        .map_err(internal_error)?;
    let patterns = task
        .goal_contract
        .as_deref()
        .map(|contract| contract.protected_paths.clone())
        .unwrap_or_default();
    let mut hunks = crate::diffmodel::build_hunks(&diff_text, &patterns);
    // 로컬과 같은 판정을 써야 한다 — 원격에서만 커밋된 hunk가 조작 가능해지면 안 된다.
    if range != crate::worktree::DiffRange::Uncommitted && !hunks.is_empty() {
        let pending = worktree
            .diff_unified_range(3, crate::worktree::DiffRange::Uncommitted)
            .map_err(internal_error)?;
        let pending = crate::diffmodel::build_hunks(&pending, &patterns);
        for hunk in hunks.iter_mut() {
            hunk.committed = !pending
                .iter()
                .any(|other| crate::diffmodel::overlaps(hunk, other));
        }
    }
    Ok(hunks)
}

#[derive(Deserialize)]
struct AnnotationSaveRequest {
    #[serde(default)]
    id: Option<String>,
    hunk_id: String,
    path: String,
    line: i64,
    side: String,
    body_md: String,
}

#[derive(Deserialize)]
struct AnnotationsResendRequest {
    ids: Vec<String>,
}

/// 주석 목록 — local `annotations_list` command와 동일하게 현재 diff에 재매칭해 반환.
async fn task_annotations_list(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Query(query): Query<DiffRangeQuery>,
) -> Result<Json<Vec<crate::annotations::RematchedAnnotation>>, (StatusCode, String)> {
    let task = db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or_else(not_found)?;
    let stored = crate::annotations::list_by_task(&state.pool, id)
        .await
        .map_err(internal_error)?;
    // 로컬 `annotations_list`와 같은 이유로 화면과 같은 범위를 써야 한다.
    let hunks = task_hunks_range(&state, &task, query.range)?;
    Ok(Json(crate::annotations::rematch(&stored, &hunks)))
}

/// draft 주석 생성/저장(onBlur 자동 저장) — local `annotation_save` command와 동일 계약.
async fn task_annotation_save(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<AnnotationSaveRequest>,
) -> Result<Json<crate::annotations::ReviewAnnotation>, (StatusCode, String)> {
    if let Some(existing_id) = request.id {
        crate::annotations::update_draft_body(&state.pool, &existing_id, &request.body_md)
            .await
            .map_err(internal_error)?;
        let updated =
            crate::annotations::list_by_ids(&state.pool, id, std::slice::from_ref(&existing_id))
                .await
                .map_err(internal_error)?;
        return updated
            .into_iter()
            .next()
            .map(Json)
            .ok_or_else(|| (StatusCode::NOT_FOUND, "주석을 찾을 수 없습니다".to_string()));
    }
    crate::annotations::create_draft(
        &state.pool,
        id,
        &request.hunk_id,
        &request.path,
        request.line,
        &request.side,
        &request.body_md,
        now(),
    )
    .await
    .map(Json)
    .map_err(internal_error)
}

/// 선택한 주석 n건을 재전송 — local `annotations_resend` command와 동일 포맷·롤백 계약.
/// convo resume은 `QueueWorker::resume_conversation`(백그라운드 스폰)이 담당한다.
async fn task_annotations_resend(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<AnnotationsResendRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let task = db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or_else(not_found)?;
    let targets = crate::annotations::list_by_ids(&state.pool, id, &request.ids)
        .await
        .map_err(internal_error)?;
    if targets.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            "재전송할 주석을 찾을 수 없습니다".to_string(),
        ));
    }
    let hunks = task_hunks(&state, &task)?;
    let message = crate::annotations::format_resend(&crate::annotations::build_resend_items(
        &targets, &hunks,
    ));

    let updated = crate::annotations::mark_sent(&state.pool, id, &request.ids)
        .await
        .map_err(internal_error)?;
    if updated == 0 {
        return Err((
            StatusCode::CONFLICT,
            "재전송 가능한 초안 주석이 없습니다(이미 전송됨)".to_string(),
        ));
    }
    if let Err(error) = state.queue.resume_conversation(id, message, now()).await {
        let _ = crate::annotations::mark_draft(&state.pool, id, &request.ids).await;
        return Err((StatusCode::CONFLICT, error));
    }
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct PartialApplyRequest {
    hunk_ids: Vec<String>,
}

#[derive(Serialize)]
struct PartialApplyResponse {
    checkpoint: String,
    kept_hunk_ids: Vec<String>,
    discarded_hunk_ids: Vec<String>,
}

/// hunk 부분 승인(B-2) — local `partial_apply` command와 동일 계약(`partial::apply` 공유,
/// 성공/실패 모두 `partial_apply` 이벤트로 남겨 적용 성공률(KPI Tech)을 관측).
async fn task_partial_apply(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<PartialApplyRequest>,
) -> Result<Json<PartialApplyResponse>, (StatusCode, String)> {
    let locks = state.queue.worktree_locks();
    let task = db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or_else(not_found)?;
    let root = authorized_repository(&state, &task.worktree_path)?;
    let _mutation = crate::runner::review_process::claim_task_mutation(
        &state.review_claims,
        &state.pool,
        &locks,
        id,
        &root,
    )
    .await
    .map_err(invalid_request)?;
    if task.state != db::state::AWAITING_REVIEW {
        return Err((
            StatusCode::CONFLICT,
            "검토 대기 중인 작업만 부분 적용할 수 있습니다".to_string(),
        ));
    }
    let hunks = task_hunks(&state, &task)?;
    let worktree = crate::worktree::Worktree {
        repo: task.repo.clone().into(),
        path: root,
        branch: task.branch.clone(),
        base: task.base.clone(),
        base_revision: task.base_revision.clone(),
    };
    let result = crate::partial::apply(&worktree, &hunks, &request.hunk_ids);
    let _ = db::append_event(
        &state.pool,
        id,
        "partial_apply",
        Some(&partial_apply_kpi_detail(&result)),
        now(),
    )
    .await;
    let outcome = result.map_err(|error| (StatusCode::CONFLICT, error.to_string()))?;

    crate::partial::save_checkpoint(&state.pool, id, &outcome.checkpoint, now())
        .await
        .map_err(internal_error)?;
    Ok(Json(PartialApplyResponse {
        checkpoint: outcome.checkpoint,
        kept_hunk_ids: outcome.kept_hunk_ids,
        discarded_hunk_ids: outcome.discarded_hunk_ids,
    }))
}

/// KPI Tech(적용 성공률) 관측용 이벤트 상세 — local `commands::partial_apply_kpi_detail`과 동일
/// 로직(모듈 경계상 별도 정의 — `task_hunks`처럼 local/runner가 각자 얇게 재구현하는 기존 관행).
fn partial_apply_kpi_detail(
    result: &Result<crate::partial::ApplyOutcome, crate::partial::PartialError>,
) -> String {
    match result {
        Ok(o) => format!(
            "ok kept={} discarded={}",
            o.kept_hunk_ids.len(),
            o.discarded_hunk_ids.len()
        ),
        Err(crate::partial::PartialError::ApplyConflict(ids)) => {
            format!("conflict failed={}", ids.len())
        }
        Err(crate::partial::PartialError::ProtectedHunkRejected(ids)) => {
            format!("protected_rejected count={}", ids.len())
        }
        Err(e) => format!("error {e}"),
    }
}

/// hunk 부분 승인 롤백 — local `partial_rollback` command와 동일 계약.
async fn task_partial_rollback(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let locks = state.queue.worktree_locks();
    let task = db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or_else(not_found)?;
    let root = authorized_repository(&state, &task.worktree_path)?;
    let _mutation = crate::runner::review_process::claim_task_mutation(
        &state.review_claims,
        &state.pool,
        &locks,
        id,
        &root,
    )
    .await
    .map_err(invalid_request)?;
    let checkpoint = crate::partial::get_checkpoint(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "되돌릴 부분 적용 체크포인트가 없습니다".to_string(),
            )
        })?;
    let worktree = crate::worktree::Worktree {
        repo: task.repo.into(),
        path: root,
        branch: task.branch,
        base: task.base,
        base_revision: task.base_revision,
    };
    crate::partial::rollback(&worktree, &checkpoint)
        .map_err(|error| (StatusCode::CONFLICT, error.to_string()))?;
    crate::partial::clear_checkpoint(&state.pool, id)
        .await
        .map_err(internal_error)?;
    let _ = db::append_event(&state.pool, id, "partial_rollback", None, now()).await;
    Ok(StatusCode::NO_CONTENT)
}

/// ensemble 후보×파일×hunk 매트릭스(B-3) — local `ensemble_matrix` command와 동일 계약
/// (`ensemble::matrix` 공유, 겹치는 hunk를 배타 그룹으로 반환).
async fn ensemble_matrix(
    State(state): State<RunnerHttpState>,
    Path(ensemble): Path<String>,
) -> Result<Json<crate::ensemble::EnsembleMatrix>, (StatusCode, String)> {
    let candidates = ensemble_candidate_hunks(&state, &ensemble).await?;
    Ok(Json(crate::ensemble::matrix(&candidates)))
}

#[derive(Deserialize)]
struct EnsembleComposeRequest {
    winner_task_id: i64,
    selections: Vec<crate::ensemble::HunkRef>,
}

/// ensemble 조합 병합(B-3) — local `ensemble_compose` command와 동일 계약(`ensemble::compose` 공유,
/// 체크포인트는 `partial_checkpoints`에 영속해 기존 `/partial/rollback`으로 되돌릴 수 있다).
async fn ensemble_compose(
    State(state): State<RunnerHttpState>,
    Path(ensemble): Path<String>,
    Json(request): Json<EnsembleComposeRequest>,
) -> Result<Json<crate::ensemble::ComposeOutcome>, (StatusCode, String)> {
    let tasks = db::tasks_by_ensemble(&state.pool, &ensemble)
        .await
        .map_err(internal_error)?;
    let winner = tasks
        .iter()
        .find(|t| t.id == request.winner_task_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "추천 후보를 찾을 수 없습니다".to_string(),
            )
        })?;
    let locks = state.queue.worktree_locks();
    let root = authorized_repository(&state, &winner.worktree_path)?;
    let _mutation = crate::runner::review_process::claim_task_mutation(
        &state.review_claims,
        &state.pool,
        &locks,
        request.winner_task_id,
        &root,
    )
    .await
    .map_err(invalid_request)?;
    if winner.state != db::state::AWAITING_REVIEW {
        return Err((
            StatusCode::CONFLICT,
            "검토 대기 중인 후보에만 조합을 적용할 수 있습니다".to_string(),
        ));
    }
    let mut candidates = Vec::new();
    for t in &tasks {
        candidates.push((t.id, task_hunks(&state, t)?));
    }
    let worktree = crate::worktree::Worktree {
        repo: winner.repo.clone().into(),
        path: root,
        branch: winner.branch.clone(),
        base: winner.base.clone(),
        base_revision: winner.base_revision.clone(),
    };
    let result = crate::ensemble::compose(
        request.winner_task_id,
        &worktree,
        &candidates,
        &request.selections,
    );
    let _ = db::append_event(
        &state.pool,
        request.winner_task_id,
        "ensemble_compose",
        Some(&compose_kpi_detail(&result)),
        now(),
    )
    .await;
    let outcome = result.map_err(|error| (StatusCode::CONFLICT, error.to_string()))?;
    crate::partial::save_checkpoint(
        &state.pool,
        request.winner_task_id,
        &outcome.checkpoint,
        now(),
    )
    .await
    .map_err(internal_error)?;
    Ok(Json(outcome))
}

/// KPI Tech(조합 적용 성공률) 관측용 이벤트 상세 — local `commands::compose_kpi_detail`과 동일 로직
/// (모듈 경계상 별도 정의 — `task_hunks`처럼 local/runner가 각자 얇게 재구현하는 기존 관행).
fn compose_kpi_detail(
    result: &Result<crate::ensemble::ComposeOutcome, crate::ensemble::ComposeError>,
) -> String {
    match result {
        Ok(o) => format!("ok applied={}", o.applied.len()),
        Err(crate::ensemble::ComposeError::ApplyConflict(ids)) => {
            format!("conflict failed={}", ids.len())
        }
        Err(crate::ensemble::ComposeError::ExclusiveGroupViolation(ids)) => {
            format!("exclusive_violation count={}", ids.len())
        }
        Err(crate::ensemble::ComposeError::ProtectedHunkRejected(ids)) => {
            format!("protected_rejected count={}", ids.len())
        }
        Err(e) => format!("error {e}"),
    }
}

/// ensemble 후보 목록 조회 + task별 구조화 hunk 조회를 묶은 헬퍼 — matrix 핸들러 전용.
async fn ensemble_candidate_hunks(
    state: &RunnerHttpState,
    ensemble: &str,
) -> Result<Vec<crate::ensemble::CandidateHunks>, (StatusCode, String)> {
    let tasks = db::tasks_by_ensemble(&state.pool, ensemble)
        .await
        .map_err(internal_error)?;
    let mut candidates = Vec::new();
    for t in &tasks {
        candidates.push((t.id, task_hunks(state, t)?));
    }
    Ok(candidates)
}

/// Quick Open(⌘K) tasks/sessions 검색 — local(Tauri command)과 동일한 `db::quickopen_search`를
/// 공유해 parity를 보장한다. 파일/스킬/커맨드 소스는 프론트가 별도 엔드포인트로 조회.
async fn quickopen(
    State(state): State<RunnerHttpState>,
    Query(query): Query<QuickOpenQuery>,
) -> Result<Json<Vec<db::QuickOpenCandidate>>, (StatusCode, String)> {
    let scopes: Vec<String> = query
        .scopes
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    db::quickopen_search(&state.pool, &query.query, &scopes, 50)
        .await
        .map(Json)
        .map_err(internal_error)
}

/// 디렉터리 스캔은 동기 파일시스템 작업이라 async 워커에서 직접 돌리면 그 워커가 스캔이
/// 끝날 때까지 묶인다 — 앱 부팅처럼 요청이 몰리는 순간 다른 요청(`/v1/tasks` 등)까지 함께
/// 지연됐다. 블로킹 풀로 넘겨 런타임을 막지 않게 한다.
async fn repositories(State(state): State<RunnerHttpState>) -> Json<Vec<String>> {
    let roots = state.config.repository_roots.clone();
    let list = tokio::task::spawn_blocking(move || discover_repositories(&roots))
        .await
        .unwrap_or_default();
    Json(list)
}

/// 선택 가능한 repository 상한 — 대형 홈 디렉터리를 root로 잡아도 응답이 폭주하지 않게.
const REPOSITORY_LIST_LIMIT: usize = 100;
/// 스캔 예산 — 방문하는 디렉터리 수 상한. 깊이 제한 대신 이 예산이 비용을 막는다.
/// repository 안으로는 내려가지 않으므로 대부분의 대형 트리(레포 내부)는 애초에 방문하지
/// 않고, git 아닌 거대 트리(예: go module cache)만 이 예산에 걸린다.
const REPOSITORY_SCAN_DIR_BUDGET: usize = 10_000;

/// 각 configured root에서 작업 대상으로 선택 가능한 git repository를 발견한다.
/// root 자신이 repository면 그대로 포함하고, 아니면 하위를 깊이 제한 없이 너비 우선으로
/// 스캔한다 — 얕은 repository를 전부 찾은 뒤에야 깊은 곳을 방문하므로, 예산이 소진돼도
/// 실사용 레포는 이미 발견된 상태다. repository 내부로는 더 내려가지 않으며(중첩 repo·
/// worktree 제외), 숨김 디렉터리와 symlink는 건너뛴다. 접근 통제는 여기가 아니라 task
/// 생성 시 `authorize_repository_path`가 수행한다 — 이 목록은 표시용 발견이다.
/// 스캔 중 "여기가 repository 루트인가"를 판정한다.
///
/// `git rev-parse`(프로세스 spawn)를 쓰지 않는다 — 예산만큼(최대 1만) 프로세스를 띄우면
/// 스캔이 수 초로 늘어난다. 루트 판정에는 `.git` 존재 확인으로 충분하고 더 정확하다:
/// `rev-parse`는 레포 **내부** 디렉터리에서도 참이라 하위 디렉터리를 루트로 오인한다.
/// worktree는 `.git`이 파일이므로 `exists()`로 본다(디렉터리 한정 아님).
fn is_repository_root(dir: &std::path::Path) -> bool {
    dir.join(".git").exists()
}

fn discover_repositories(roots: &[std::path::PathBuf]) -> Vec<String> {
    let mut found: Vec<std::path::PathBuf> = Vec::new();
    let mut queue: std::collections::VecDeque<std::path::PathBuf> = roots.iter().cloned().collect();
    let mut visited = 0usize;
    while let Some(dir) = queue.pop_front() {
        if found.len() >= REPOSITORY_LIST_LIMIT || visited >= REPOSITORY_SCAN_DIR_BUDGET {
            break;
        }
        visited += 1;
        if is_repository_root(&dir) {
            found.push(dir);
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<std::path::PathBuf> = entries
            .flatten()
            // symlink 디렉터리는 file_type이 symlink로 잡혀 자연히 제외된다(순환·경계 이탈 방지).
            .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| !name.starts_with('.'))
                    .unwrap_or(true)
            })
            .collect();
        children.sort();
        queue.extend(children);
    }
    let mut list: Vec<String> = found
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    list.sort();
    list.dedup();
    list.truncate(REPOSITORY_LIST_LIMIT);
    list
}

/// GitHub 이슈 목록(C-2, local `github_issues_list` command와 동일 계약) — 비 GitHub 레포는
/// `NotGithubRepo`(섹션 숨김), gh 미설치/미인증은 `Unavailable`(안내 카드)로 반환.
async fn github_issues(
    State(state): State<RunnerHttpState>,
    Query(query): Query<GithubIssuesQuery>,
) -> Result<Json<GithubIssuesResponse>, (StatusCode, String)> {
    let root = authorized_repository(&state, &query.repository)?;
    let Some(owner_repo) = crate::github::remote_owner_repo(&root) else {
        return Ok(Json(GithubIssuesResponse::NotGithubRepo));
    };
    match crate::github::list_issues(&root) {
        Ok(issues) => Ok(Json(GithubIssuesResponse::Ready { owner_repo, issues })),
        Err(crate::github::GhError::GhUnavailable) => Ok(Json(GithubIssuesResponse::Unavailable)),
        Err(e @ crate::github::GhError::CommandFailed(_)) => Err(invalid_request(e.to_string())),
    }
}

/// 후보 경로 중 이슈를 볼 수 있는 레포만(local `github_repos_list`와 동일 계약). 허용되지 않은
/// 경로는 거부가 아니라 조용히 제외한다 — 홈은 로컬 최근 작업 경로를 그대로 넘기므로,
/// 하나가 화이트리스트 밖이라고 레포 버튼 전체가 사라지면 안 된다.
async fn github_repos(
    State(state): State<RunnerHttpState>,
    Json(request): Json<GithubReposRequest>,
) -> Json<Vec<crate::github::GhRepo>> {
    let authorized: Vec<String> = request
        .repositories
        .iter()
        .filter_map(|repository| authorized_repository(&state, repository).ok())
        .map(|root| root.to_string_lossy().into_owned())
        .collect();
    Json(crate::github::resolve_repos(&authorized))
}

/// 이슈 번호로 큐 태스크 생성(C-2, local `github_create_task_from_issue`와 동일 계약) —
/// 지시문은 제목+본문+`#N` 참조, 생성 후 `owner/repo#N`을 `task_issue_refs`에 저장.
async fn github_issue_task_create(
    State(state): State<RunnerHttpState>,
    Json(request): Json<GithubIssueTaskCreateRequest>,
) -> Result<Json<db::Task>, (StatusCode, String)> {
    let root = authorized_repository(&state, &request.repository)?;
    let owner_repo = crate::github::remote_owner_repo(&root)
        .ok_or_else(|| invalid_request("GitHub 레포가 아닙니다".to_string()))?;
    let detail = crate::github::view_issue(&root, request.number)
        .map_err(|e| invalid_request(e.to_string()))?;
    let instruction = crate::github::build_instruction(&owner_repo, request.number, &detail);
    let queued = crate::runner::QueuedTaskRequest {
        repository: root.to_string_lossy().into_owned(),
        instruction,
        agent: request.agent,
        role: crate::agent::DEFAULT_ROLE.to_string(),
        model: String::new(),
        reasoning_effort: String::new(),
        mode: "terminal".to_string(),
        goal_contract: None,
        resume_session: None,
    };
    let task = crate::runner::create_queued_task(
        &state.config,
        &state.pool,
        &state.queue.worktree_locks(),
        queued,
        now(),
    )
    .await
    .map_err(create_task_error_response)?;
    let issue_ref = format!("{owner_repo}#{}", request.number);
    let _ = crate::github::set_issue_ref(&state.pool, task.id, &issue_ref).await;
    Ok(Json(task))
}

/// 이슈 삭제(local `github_issue_delete` command와 동일 계약) — **close가 아니라 완전 삭제**다.
/// 권한 부족·미존재는 gh의 사유를 그대로 400으로 올린다. gh 부재도 마찬가지로 실패로 다룬다
/// — 목록 조회와 달리 "조용히 비활성"이 성립하지 않는 동작이라, 안 지워졌으면 그렇게 말해야 한다.
async fn github_issue_delete(
    State(state): State<RunnerHttpState>,
    Query(query): Query<GithubIssueDeleteQuery>,
) -> Result<StatusCode, (StatusCode, String)> {
    let root = authorized_repository(&state, &query.repository)?;
    crate::github::delete_issue(&root, query.number).map_err(|e| invalid_request(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// 경로가 git 저장소인지 — 아니면 작업은 격리 없이 직접 모드로 실행된다.
async fn git_status(
    State(state): State<RunnerHttpState>,
    Query(query): Query<PathQuery>,
) -> Result<Json<GitStatusResponse>, (StatusCode, String)> {
    let dir = authorized_repository(&state, &query.path)?;
    Ok(Json(GitStatusResponse {
        is_repo: crate::worktree::is_git_repository(&dir),
    }))
}

/// 폴더를 git 저장소로 초기화한다(현재 내용을 초기 커밋으로). 이미 저장소면 멱등하게 통과.
/// 사용자 폴더를 바꾸는 동작이므로 프런트가 명시적으로 요청할 때만 호출된다.
async fn git_init(
    State(state): State<RunnerHttpState>,
    Json(body): Json<PathBody>,
) -> Result<Json<GitStatusResponse>, (StatusCode, String)> {
    let dir = authorized_repository(&state, &body.path)?;
    crate::worktree::init_repository(&dir)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(GitStatusResponse { is_repo: true }))
}

/// 브라우저가 시작점으로 쓰는 configured root 목록.
async fn file_roots(State(state): State<RunnerHttpState>) -> Json<Vec<String>> {
    Json(
        state
            .config
            .repository_roots
            .iter()
            .map(|root| crate::fsapi::display_path(root))
            .collect(),
    )
}

/// 디렉터리 한 단계 나열 — 원격 파일 브라우저의 지연 로딩용.
///
/// `file_tree`와 같은 root 검증을 거치므로 configured root 밖은 볼 수 없다. 상위 경로도
/// 같은 검증을 통과할 때만 준다 — root에 도달하면 더 올라갈 수 없다.
async fn file_browse(
    State(state): State<RunnerHttpState>,
    Query(query): Query<BrowseQuery>,
) -> Result<Json<BrowseResponse>, (StatusCode, String)> {
    let requested = if query.path.trim().is_empty() {
        state
            .config
            .repository_roots
            .first()
            .map(|root| root.to_string_lossy().into_owned())
            .ok_or((
                StatusCode::NOT_FOUND,
                "repository root가 설정되어 있지 않습니다".to_string(),
            ))?
    } else {
        query.path.clone()
    };
    let dir = authorized_repository(&state, &requested)?;
    let entries = crate::fsapi::browse_dir(&dir).map_err(invalid_path)?;
    let parent =
        crate::fsapi::browse_parent(&dir).filter(|p| authorized_repository(&state, p).is_ok());
    Ok(Json(BrowseResponse {
        path: crate::fsapi::display_path(&dir),
        parent,
        entries,
    }))
}

async fn file_tree(
    State(state): State<RunnerHttpState>,
    Query(query): Query<RepositoryQuery>,
) -> Result<Json<Vec<crate::fsapi::FsNode>>, (StatusCode, String)> {
    let root = authorized_repository(&state, &query.repository)?;
    crate::fsapi::build_tree(&root)
        .map(Json)
        .map_err(internal_error)
}

/// 스킬 목록 — 원격 세션의 `/` 드롭다운을 Runner 호스트의 실측값으로 채운다.
///
/// 파일 경로와 같은 root 검증을 거치므로 configured root 밖의 저장소는 볼 수 없다.
/// 목록만 준다 — 본문은 프롬프트 확장 시점에 Runner가 직접 읽는다.
async fn skills_list(
    State(state): State<RunnerHttpState>,
    Query(query): Query<RepositoryQuery>,
) -> Result<Json<Vec<crate::skills::SkillMeta>>, (StatusCode, String)> {
    let root = authorized_repository(&state, &query.repository)?;
    Ok(Json(crate::skills::list_skills(&root.to_string_lossy())))
}

async fn file_read(
    State(state): State<RunnerHttpState>,
    Query(query): Query<FileQuery>,
) -> Result<Json<crate::fsapi::FileContent>, (StatusCode, String)> {
    let root = authorized_repository(&state, &query.repository)?;
    crate::fsapi::read_file(&root, &query.path)
        .map(Json)
        .map_err(invalid_path)
}

async fn file_write(
    State(state): State<RunnerHttpState>,
    Json(request): Json<FileWriteRequest>,
) -> Result<Json<i64>, (StatusCode, String)> {
    let root = authorized_repository(&state, &request.repository)?;
    crate::runner::file_mutation::write_file(
        &state.pool,
        &state.queue.worktree_locks(),
        &state.review_claims,
        &root,
        &request.path,
        &request.content,
    )
    .await
    .map(Json)
    .map_err(invalid_path)
}

async fn schedules(
    State(state): State<RunnerHttpState>,
) -> Result<Json<Vec<db::Schedule>>, (StatusCode, String)> {
    db::list_schedules(&state.pool)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn schedule_create(
    State(state): State<RunnerHttpState>,
    Json(request): Json<ScheduleCreateRequest>,
) -> Result<Json<i64>, (StatusCode, String)> {
    parse_schedule(&request)?;
    db::insert_schedule(
        &state.pool,
        &request.label,
        &request.cron,
        &request.kind,
        &request.payload,
        now(),
        request.tz_offset_secs,
    )
    .await
    .map(Json)
    .map_err(internal_error)
}

async fn schedule_delete(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    db::remove_schedule(&state.pool, id)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn schedule_set_enabled(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<ScheduleEnabledRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    db::set_schedule_enabled(&state.pool, id, request.enabled)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reminder_create(
    State(state): State<RunnerHttpState>,
    Json(request): Json<ReminderCreateRequest>,
) -> Result<Json<i64>, (StatusCode, String)> {
    let text = request.text.trim();
    if text.is_empty() || request.delay_minutes <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "리마인더 내용과 양의 delay_minutes가 필요합니다".to_string(),
        ));
    }
    let now = now();
    let label = text.chars().take(20).collect::<String>();
    db::insert_schedule_with_run_at(
        &state.pool,
        &label,
        "",
        "reminder",
        &serde_json::json!({ "text": text }).to_string(),
        Some(now + request.delay_minutes * 60),
        now,
        32400,
    )
    .await
    .map(Json)
    .map_err(internal_error)
}

async fn events(
    State(state): State<RunnerHttpState>,
    Query(query): Query<AfterQuery>,
) -> Result<Json<Vec<RunnerEvent>>, (StatusCode, String)> {
    state
        .events
        .replay_after(query.after.unwrap_or(0))
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn live_events(
    ws: WebSocketUpgrade,
    State(state): State<RunnerHttpState>,
    Query(query): Query<AfterQuery>,
) -> Response {
    let after = query.after.unwrap_or(0).max(0);
    // 브라우저는 요청한 서브프로토콜 중 하나를 서버가 에코하지 않으면 연결을 실패시킨다.
    // 클라이언트는 ["praxis", <pairing token>]을 보내므로 "praxis"를 선택해 응답한다.
    ws.protocols(["praxis"])
        .on_upgrade(move |socket| stream_events(socket, state.events, after))
}

async fn stream_events(mut socket: WebSocket, events: EventHub, after: i64) {
    let Ok(subscription) = events.subscribe_after(after).await else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };
    let mut last_sent = after;
    if send_subscription(&mut socket, &events, &subscription, &mut last_sent)
        .await
        .is_err()
    {
        return;
    }
    let mut receiver = subscription.receiver;
    // 모바일은 화면 잠금·셀룰러↔WiFi 전환에서 TCP가 조용히 죽는다. 주기적 ping의 응답이
    // 없으면 좀비 소켓으로 보고 끊어, 클라이언트가 재연결 경로를 타게 한다. (설계 0013 §7.3)
    let mut keepalive = tokio::time::interval(ping_interval());
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    keepalive.tick().await; // 최초 tick은 즉시 도래하므로 흘려보낸다.
    let mut awaiting_pong = false;
    loop {
        tokio::select! {
            message = socket.next() => {
                match message {
                    None | Some(Err(_)) => return,
                    Some(Ok(Message::Pong(_))) => awaiting_pong = false,
                    Some(Ok(_)) => {}
                }
            }
            _ = keepalive.tick() => {
                if awaiting_pong {
                    return;
                }
                if socket.send(Message::Ping(Vec::new())).await.is_err() {
                    return;
                }
                awaiting_pong = true;
            }
            received = receiver.recv() => match received {
                Ok(event) if event.sequence > last_sent => {
                    if send_event(&mut socket, &event).await.is_err() {
                        return;
                    }
                    last_sent = event.sequence;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let Ok(replay) = events.replay_after(last_sent).await else { return };
                    for event in replay {
                        if event.sequence > last_sent && send_event(&mut socket, &event).await.is_ok() {
                            last_sent = event.sequence;
                        } else if event.sequence > last_sent {
                            return;
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    }
}

async fn send_subscription(
    socket: &mut WebSocket,
    events: &EventHub,
    subscription: &ReplaySubscription,
    last_sent: &mut i64,
) -> Result<(), axum::Error> {
    send_json(
        socket,
        &Watermark {
            kind: "watermark",
            sequence: subscription.watermark,
        },
    )
    .await?;
    for event in &subscription.replay {
        if event.sequence > *last_sent {
            send_event(socket, event).await?;
            *last_sent = event.sequence;
        }
    }
    // 첫 replay는 REPLAY_LIMIT에서 잘린다. 그대로 live로 넘어가면 마지막 replay와 watermark
    // 사이 이벤트가 영영 전달되지 않는다(broadcast receiver는 구독 이후 것만 나른다).
    // 오래 오프라인이었던 모바일이 정확히 이 경우다 — watermark까지 페이지로 마저 보낸다.
    while *last_sent < subscription.watermark {
        let page = events
            .replay_through(*last_sent, subscription.watermark)
            .await
            .map_err(axum::Error::new)?;
        if page.is_empty() {
            break;
        }
        for event in page {
            if event.sequence > *last_sent {
                send_event(socket, &event).await?;
                *last_sent = event.sequence;
            }
        }
    }
    Ok(())
}

/// WebSocket keepalive 주기. 테스트는 짧게 덮어써 좀비 소켓 종료를 검증한다.
fn ping_interval() -> std::time::Duration {
    const DEFAULT_SECS: u64 = 20;
    std::env::var("PRAXIS_RUNNER_WS_PING_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map_or_else(
            || std::time::Duration::from_secs(DEFAULT_SECS),
            std::time::Duration::from_secs,
        )
}

async fn send_event(socket: &mut WebSocket, event: &RunnerEvent) -> Result<(), axum::Error> {
    send_json(socket, event).await
}

async fn send_json<T: Serialize>(socket: &mut WebSocket, value: &T) -> Result<(), axum::Error> {
    let body = serde_json::to_string(value).map_err(axum::Error::new)?;
    socket.send(Message::Text(body)).await
}

fn internal_error(error: anyhow::Error) -> (StatusCode, String) {
    eprintln!("Runner HTTP 처리 실패: {error}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Runner 내부 오류".to_string(),
    )
}

fn authorized_repository(
    state: &RunnerHttpState,
    requested: &str,
) -> Result<std::path::PathBuf, (StatusCode, String)> {
    crate::runner::auth::authorize_repository_path(
        &state.config.repository_roots,
        std::path::Path::new(requested),
    )
    .map_err(|_| {
        (
            StatusCode::FORBIDDEN,
            "허용되지 않는 repository 경로입니다".to_string(),
        )
    })
}

fn invalid_path(_: anyhow::Error) -> (StatusCode, String) {
    (
        StatusCode::BAD_REQUEST,
        "유효하지 않은 파일 경로입니다".to_string(),
    )
}

fn invalid_request(error: String) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, error)
}

fn parse_schedule(request: &ScheduleCreateRequest) -> Result<(), (StatusCode, String)> {
    use std::str::FromStr;
    cron::Schedule::from_str(&request.cron)
        .map(|_| ())
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    if !matches!(
        request.kind.as_str(),
        "task" | "reminder" | "quiz" | "retro"
    ) {
        return Err((
            StatusCode::BAD_REQUEST,
            "알 수 없는 스케줄 종류입니다".to_string(),
        ));
    }
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn not_found() -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, "task를 찾을 수 없습니다".to_string())
}

#[cfg(test)]
mod discovery_tests {
    use super::*;

    fn tmp_root(tag: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = crate::testtmp::dir().join(format!(
            "praxis-discover-{}-{}-{}",
            tag,
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_repository_roots_without_descending_into_them() {
        let root = tmp_root("basic");
        // 레포 루트(디렉터리형 .git)
        std::fs::create_dir_all(root.join("alpha/.git")).unwrap();
        // 레포 내부의 하위 디렉터리는 루트로 잡히면 안 된다.
        std::fs::create_dir_all(root.join("alpha/src")).unwrap();
        // worktree는 .git이 파일이다.
        std::fs::create_dir_all(root.join("beta")).unwrap();
        std::fs::write(root.join("beta/.git"), "gitdir: /elsewhere").unwrap();
        // 그냥 폴더
        std::fs::create_dir_all(root.join("plain/nested")).unwrap();

        let found = discover_repositories(std::slice::from_ref(&root));
        let names: Vec<String> = found
            .iter()
            .map(|p| p.rsplit(['/', '\\']).next().unwrap().to_string())
            .collect();

        assert!(names.contains(&"alpha".to_string()));
        assert!(
            names.contains(&"beta".to_string()),
            "worktree(.git 파일)도 레포"
        );
        assert!(
            !names.contains(&"src".to_string()),
            "레포 내부로 내려가지 않는다"
        );
        assert!(!names.contains(&"plain".to_string()));
        assert!(!names.contains(&"nested".to_string()));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn root_itself_can_be_a_repository() {
        let root = tmp_root("self");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let found = discover_repositories(std::slice::from_ref(&root));
        assert_eq!(found.len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn hidden_directories_are_skipped() {
        let root = tmp_root("hidden");
        std::fs::create_dir_all(root.join(".cache/repo/.git")).unwrap();
        assert!(discover_repositories(std::slice::from_ref(&root)).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
