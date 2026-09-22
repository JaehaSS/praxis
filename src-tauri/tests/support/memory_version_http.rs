use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use praxis_lib::db;
use praxis_lib::runner::auth::RunnerAuth;
use praxis_lib::runner::config::{ExecutionPolicy, RunnerConfig};
use praxis_lib::runner::events::EventHub;
use praxis_lib::runner::http::{self, RunnerHttpState};
use praxis_lib::runner::queue::QueueWorker;

const TOKEN: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

pub struct Fixture {
    address: SocketAddr,
    pub pool: sqlx::SqlitePool,
    pub root: std::path::PathBuf,
    #[allow(dead_code)]
    // scope test에서만 사용하며 각 integration target은 support를 별도 컴파일한다.
    pub outside: std::path::PathBuf,
    server: tokio::task::JoinHandle<()>,
    base: std::path::PathBuf,
}

impl Fixture {
    #[allow(dead_code)]
    pub async fn start() -> Self {
        Self::start_server(None).await
    }

    #[allow(dead_code)]
    pub async fn start_with_restore_barrier() -> Self {
        Self::start_server(Some(Arc::new(tokio::sync::Barrier::new(2)))).await
    }

    async fn start_server(restore_barrier: Option<Arc<tokio::sync::Barrier>>) -> Self {
        // fixture마다 고유 경로 — 프로세스 id만 쓰면 한 바이너리 안의 병렬 테스트가
        // 같은 SQLite 파일을 열고 서로의 디렉터리를 지워 "database is locked"로 죽는다.
        static SEQUENCE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let base = super::temp_root::dir().join(format!(
            "praxis-memory-version-http-{}-{sequence}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let root = root.canonicalize().unwrap();
        let database = base.join("memory.sqlite");
        let pool = db::init_pool(database.to_str().unwrap()).await.unwrap();
        praxis_lib::memory::migrate(&pool).await.unwrap();
        let token_path = base.join("runner.token");
        write_token(&token_path);
        let config = RunnerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            repository_roots: vec![root.clone()],
            max_concurrent_tasks: 1,
            execution_policy: ExecutionPolicy::AlwaysApprove,
            pairing_token_file: token_path.clone(),
        };
        let state = RunnerHttpState {
            auth: RunnerAuth::from_file(&token_path).unwrap(),
            events: EventHub::start(pool.clone()),
            pool: pool.clone(),
            config,
            recovered_tasks: 0,
            queue: QueueWorker::new(pool.clone(), 1),
            review_claims: Default::default(),
            started_at: 0,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = http::router(state);
        let router = match restore_barrier {
            Some(barrier) => router.layer(axum::middleware::from_fn(
                move |request: Request, next: Next| {
                    wait_at_restore_barrier(request, next, Arc::clone(&barrier))
                },
            )),
            None => router,
        };
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self {
            address,
            pool,
            root,
            outside,
            server,
            base,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }
}

async fn wait_at_restore_barrier(
    request: Request,
    next: Next,
    barrier: Arc<tokio::sync::Barrier>,
) -> Response {
    let path = request.uri().path();
    if request.method() == axum::http::Method::POST
        && path.contains("/versions/")
        && path.ends_with("/restore")
    {
        barrier.wait().await;
    }
    next.run(request).await
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

pub fn authenticated_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {TOKEN}").parse().unwrap(),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

fn write_token(path: &std::path::Path) {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, TOKEN).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}
