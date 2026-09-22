use std::env;
use std::fs;
use std::time::Duration;

use praxis_lib::runner::{
    self,
    auth::RunnerAuth,
    config::RunnerConfig,
    events::EventHub,
    http::{self, RunnerHttpState},
    queue::QueueWorker,
};

fn main() -> Result<(), String> {
    // systemd/ssh 기동은 최소 PATH만 받는다 — 에이전트 CLI(claude/codex/agy) 해석 전에 보강.
    praxis_lib::envpath::augment_path();
    let config_path = env::var("PRAXIS_RUNNER_CONFIG")
        .map_err(|_| "PRAXIS_RUNNER_CONFIG가 필요합니다".to_string())?;
    let config_text = fs::read_to_string(&config_path)
        .map_err(|error| format!("Runner 설정을 읽을 수 없습니다: {error}"))?;
    let config = RunnerConfig::from_toml(&config_text)?;
    let db_path =
        env::var("PRAXIS_RUNNER_DB").unwrap_or_else(|_| "praxis-runner.sqlite".to_string());
    // 임베딩 캐시를 DB 옆에 고정한다 — 기본값은 CWD 상대 경로라 systemd/ssh가 어디서
    // 띄우느냐에 따라 모델을 다시 내려받는다. 절대화 전에는 부모를 신뢰할 수 없다.
    if let Some(parent) = std::path::absolute(&db_path)
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
    {
        let cache = parent.join("fastembed");
        if fs::create_dir_all(&cache).is_ok() {
            praxis_lib::embed::set_cache_dir(cache);
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let runner = runtime
        .block_on(runner::initialize(config, &db_path, now()))
        .map_err(|error| error.to_string())?;
    eprintln!(
        "Praxis Runner initialized on {} (max {} tasks, recovered {})",
        runner.config().bind,
        runner.config().max_concurrent_tasks,
        runner.recovered_tasks()
    );
    let worker = QueueWorker::with_worktree_locks(
        runner.pool().clone(),
        runner.config().max_concurrent_tasks,
        runner.worktree_locks(),
    );
    let auth = RunnerAuth::from_file(&runner.config().pairing_token_file)?;
    let worktree_locks = worker.worktree_locks();
    runtime.block_on(async move {
        // Opt-in only. Migration and owned-container recovery finish while the
        // Runner instance lock is held and before either queue can dispatch.
        let workflow = match env::var_os("PRAXIS_WORKFLOW_CONFIG") {
            Some(path) => Some(runner::workflow::WorkflowService::open(
                std::path::Path::new(&path), runner.config(),
                std::path::Path::new(&db_path), worker.capacity(),
            ).await.map_err(|error| format!("Workflow initialization failed: {error}"))?),
            None => None,
        };
        if env::var_os("PRAXIS_WORKFLOW_CHECK_ONLY").is_some() {
            let service=workflow.as_ref().ok_or("PRAXIS_WORKFLOW_CONFIG is required for the workflow check")?;
            let result=service.check_configured_profiles().await;
            if let Err(error)=result {
                service.shutdown().await.map_err(|cleanup|format!("Workflow check cleanup requires recovery: {cleanup}"))?;
                return Err(error.to_string());
            }
            println!("{}",serde_json::json!({"schema_version":1,"status":"verified","profiles":result.unwrap()}));
            service.shutdown().await.map_err(|error|error.to_string())?;
            return Ok(());
        }
        // EventHub::start는 내부에서 tokio::spawn을 호출하므로 런타임 컨텍스트 안에서 만들어야 한다.
        let events = EventHub::start(runner.pool().clone());
        let state = RunnerHttpState {
            auth,
            pool: runner.pool().clone(),
            config: runner.config().clone(),
            recovered_tasks: runner.recovered_tasks(),
            events,
            queue: worker.clone(),
            started_at: runner::now_secs(),
            review_claims: Default::default(),
        };
        tokio::spawn(run_queue(worker));
        // Web Push는 EventHub 구독으로 붙는다 — 상태 전이 호출부를 건드리지 않는 단일 지점.
        // 키를 만들지 못하면 알림만 조용히 빠지고 나머지 기능은 그대로 돈다.
        match runner::push::VapidKeys::load_or_create(&runner::push::default_key_path()) {
            Ok(keys) => {
                tokio::spawn(runner::push::watch(
                    runner.pool().clone(),
                    state.events.clone(),
                    keys,
                ));
            }
            Err(error) => eprintln!("Web Push 비활성 — VAPID 키를 준비하지 못했습니다: {error}"),
        }
        tokio::spawn(runner::schedule::tick_loop(
            runner.config().clone(),
            runner.pool().clone(),
            worktree_locks,
        ));
        tokio::spawn(run_retention(runner.pool().clone()));
        let listener = tokio::net::TcpListener::bind(state.config.bind)
            .await
            .map_err(|error| format!("Runner HTTP bind 실패: {error}"))?;
        let (draining, drained) = tokio::sync::oneshot::channel();
        let workflow_loop = workflow.as_ref().map(|service| tokio::spawn(service.clone().run_loop()));
        let server = axum::serve(
            listener,
            http::router_with_workflow(state, workflow.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            let _ = draining.send(());
        });
        // SSE clients may hold a response open indefinitely. Bound the drain;
        // side children are already reaped by shutdown_signal before it starts.
        let served = tokio::select! {
            result = std::future::IntoFuture::into_future(server) =>
                result.map_err(|error| format!("Runner HTTP server가 종료되었습니다: {error}")),
            _ = async {
                let _ = drained.await;
                tokio::time::sleep(Duration::from_secs(3)).await;
            } => Ok(()),
        };
        praxis_lib::side_question::shutdown_all();
        if let Some(workflow) = workflow {
            let stopped = workflow.shutdown().await;
            if let Some(task) = workflow_loop { task.await.map_err(|error| format!("Workflow supervisor join failed: {error}"))?; }
            stopped.map_err(|error| format!("Workflow shutdown requires recovery: {error}"))?;
        }
        served
    })
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {},
                    _ = terminate.recv() => {},
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    // Stop side children immediately, before waiting for HTTP connections to drain.
    praxis_lib::side_question::shutdown_all();
}

async fn run_retention(pool: sqlx::SqlitePool) {
    let mut interval = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
    loop {
        interval.tick().await;
        if let Err(error) = runner::prune_history(&pool, now()).await {
            eprintln!("Runner retention cleanup 실패: {error}");
        }
    }
}

async fn run_queue(worker: QueueWorker) {
    loop {
        loop {
            match worker.spawn_next(now()).await {
                Ok(true) => continue,
                Ok(false) => break,
                Err(error) => {
                    eprintln!("Runner queue 작업 시작 실패: {error}");
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
