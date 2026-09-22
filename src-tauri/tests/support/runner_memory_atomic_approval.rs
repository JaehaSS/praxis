use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use praxis_lib::runner::auth::RunnerAuth;
use praxis_lib::runner::config::{ExecutionPolicy, RunnerConfig};
use praxis_lib::runner::events::EventHub;
use praxis_lib::runner::http::{self, RunnerHttpState};
use praxis_lib::runner::queue::QueueWorker;
use sqlx::SqlitePool;

static NEXT_FILE: AtomicU32 = AtomicU32::new(1);
const TEST_TOKEN: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

pub struct RunnerApprovalFixture {
    pub app: axum::Router,
    pub memory_id: i64,
    pool: SqlitePool,
    database_path: String,
    repository_root: std::path::PathBuf,
    token_path: String,
}

impl RunnerApprovalFixture {
    pub async fn new() -> Self {
        let serial = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let database_path = temporary_path("database", serial);
        let repository_root = std::path::PathBuf::from(temporary_path("root", serial));
        std::fs::create_dir_all(&repository_root).unwrap();
        let repository_root = repository_root.canonicalize().unwrap();
        let pool = praxis_lib::db::init_pool(&database_path).await.unwrap();
        praxis_lib::memory::migrate(&pool).await.unwrap();
        let memory_id = create_candidate(&pool, &repository_root).await;
        let token_path = write_token_file(serial);
        let app = router(pool.clone(), repository_root.clone(), &token_path);
        Self {
            app,
            memory_id,
            pool,
            database_path,
            repository_root,
            token_path,
        }
    }

    pub async fn cleanup(self) {
        self.pool.close().await;
        for path in [
            self.database_path.clone(),
            format!("{}-wal", self.database_path),
            format!("{}-shm", self.database_path),
        ] {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_file(self.token_path);
        let _ = std::fs::remove_dir(self.repository_root);
    }

    pub async fn add_expired_confirmation(&self) {
        praxis_lib::memory::add_user_confirmation(&self.pool, self.memory_id, 1, Some(2))
            .await
            .unwrap();
    }

    pub async fn move_out_of_scope(&self) {
        sqlx::query("UPDATE memories SET scope_key = '/outside-runner-root' WHERE id = ?")
            .bind(self.memory_id)
            .execute(&self.pool)
            .await
            .unwrap();
    }

    pub async fn break_memory_lookup(&self) {
        sqlx::query("DROP TABLE memories")
            .execute(&self.pool)
            .await
            .unwrap();
    }
}

pub fn request(memory_id: i64, expected_version: i64) -> Request<Body> {
    request_with_auth(memory_id, expected_version, true)
}

pub fn request_with_auth(
    memory_id: i64,
    expected_version: i64,
    authenticated: bool,
) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("/v1/memories/{memory_id}/confirm-and-approve"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "expected_version": expected_version }).to_string(),
        ))
        .unwrap();
    if authenticated {
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {TEST_TOKEN}").parse().unwrap(),
        );
    }
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:43123".parse::<SocketAddr>().unwrap(),
    ));
    request
}

pub async fn response_json(response: axum::response::Response) -> serde_json::Value {
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

pub async fn response_text(response: axum::response::Response) -> String {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

async fn create_candidate(pool: &SqlitePool, root: &std::path::Path) -> i64 {
    praxis_lib::memory::create_candidate(
        pool,
        praxis_lib::memory::tier::PROJECT,
        Some(root.to_string_lossy().as_ref()),
        praxis_lib::memory::knowledge_type::CONVENTION,
        "runner atomic approval",
        Some("test"),
        100,
    )
    .await
    .unwrap()
}

fn router(pool: SqlitePool, root: std::path::PathBuf, token_path: &str) -> axum::Router {
    let config = RunnerConfig {
        bind: "127.0.0.1:47831".parse().unwrap(),
        repository_roots: vec![root],
        max_concurrent_tasks: 2,
        execution_policy: ExecutionPolicy::AlwaysApprove,
        pairing_token_file: token_path.into(),
    };
    let state = RunnerHttpState {
        auth: RunnerAuth::from_file(token_path.as_ref()).unwrap(),
        events: EventHub::start(pool.clone()),
        queue: QueueWorker::new(pool.clone(), 2),
        pool,
        config,
        recovered_tasks: 0,
        review_claims: Default::default(),
        started_at: 0,
    };
    http::router(state)
}

fn temporary_path(kind: &str, serial: u32) -> String {
    super::temp_root::dir()
        .join(format!(
            "praxis-atomic-approval-{kind}-{}-{serial}",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

fn write_token_file(serial: u32) -> String {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let path = temporary_path("token", serial);
    std::fs::write(&path, TEST_TOKEN).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    path
}
