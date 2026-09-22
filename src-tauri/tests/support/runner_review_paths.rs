use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{Method, Request};
use serde_json::Value;

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn temporary_dir(label: &str) -> PathBuf {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = super::temp_root::dir().join(format!(
        "praxis-runner-review-{label}-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

pub fn token_file(token: &str) -> PathBuf {
    let path = temporary_dir("token").join("pairing-token");
    std::fs::write(&path, token).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    path
}

pub fn request(
    token: &str,
    method: Method,
    uri: &str,
    body: Option<Value>,
    authenticated: bool,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if authenticated {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }
    let mut request = builder
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:43123".parse::<SocketAddr>().unwrap(),
    ));
    request
}

pub async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

pub async fn assert_process_receipt_resolved(
    pool: &sqlx::SqlitePool,
    task_id: i64,
    operation: &str,
    phase: &str,
) {
    let receipts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM review_process_receipts \
         WHERE task_id = ? AND operation = ? AND phase = ?",
    )
    .bind(task_id)
    .bind(operation)
    .bind(phase)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(receipts, 1);
    let leases: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM review_process_leases WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(leases, 0);
}
