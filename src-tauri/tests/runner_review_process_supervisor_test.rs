#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use std::ffi::OsString;

use praxis_lib::db;
use praxis_lib::managed_process;
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase, ReviewProcessState};

#[tokio::test(flavor = "multi_thread")]
async fn durable_supervisor_clears_lease_only_after_group_is_reaped() {
    let fixture = fixture("complete").await;
    let registrar = review_process::registrar(
        fixture.pool.clone(),
        fixture.task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
    )
    .unwrap();
    tokio::task::spawn_blocking(move || {
        let args = vec![OsString::from("-c"), OsString::from("printf done")];
        let mut spawned =
            managed_process::spawn_registered("/bin/sh", &args, Some(&registrar), |command| {
                command.stdout(std::process::Stdio::null());
            })
            .unwrap();
        assert!(spawned.child.wait_with_output().unwrap().status.success());
        spawned.lease.take().unwrap().complete().unwrap();
    })
    .await
    .unwrap();

    assert!(
        review_process::lease(&fixture.pool, fixture.task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn premature_completion_quarantines_instead_of_clearing_live_group() {
    let fixture = fixture("premature").await;
    let registrar = review_process::registrar(
        fixture.pool.clone(),
        fixture.task_id,
        ReviewOperation::Challenge,
        ReviewPhase::Reviewer,
    )
    .unwrap();
    let pid = tokio::task::spawn_blocking(move || {
        let args = vec![OsString::from("30")];
        let mut spawned =
            managed_process::spawn_registered("/bin/sleep", &args, Some(&registrar), |_| {})
                .unwrap();
        let pid = spawned.pid;
        assert!(spawned.lease.take().unwrap().complete().is_err());
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
        let _ = spawned.child.wait();
        pid
    })
    .await
    .unwrap();

    let lease = review_process::lease(&fixture.pool, fixture.task_id, ReviewOperation::Challenge)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.state, ReviewProcessState::Quarantined);
    assert!(lease.detail.unwrap().contains(&pid.to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn managed_timeout_reaps_group_before_resolving_lease() {
    let fixture = fixture("timeout").await;
    let registrar = review_process::registrar(
        fixture.pool.clone(),
        fixture.task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyTest,
    )
    .unwrap();
    let root = temp_root::dir();
    let result = tokio::task::spawn_blocking(move || {
        praxis_lib::verify::run_check_registered(&root, "sleep 30", 1, Some(&registrar))
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, -1);
    assert!(result.tail.contains("타임아웃"));
    let pgid: i64 = sqlx::query_scalar(
        "SELECT pgid FROM review_process_receipts \
         WHERE task_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(fixture.task_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert!(nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pgid as i32), None).is_err());
    assert!(
        review_process::lease(&fixture.pool, fixture.task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .is_none()
    );
}

struct Fixture {
    pool: sqlx::SqlitePool,
    task_id: i64,
}

async fn fixture(label: &str) -> Fixture {
    let path = temp_root::dir().join(format!(
        "praxis-review-supervisor-{label}-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    let task_id = db::insert_task(
        &pool, "/repo", "branch", "main", "/wt", "review", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    Fixture { pool, task_id }
}
