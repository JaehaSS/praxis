//! 파일형 메모리(설계 2026-09-13) 이후의 시작 게이트 규약.
//!
//! 옛 규약은 "투영 영수증이 없는 Queued 작업은 spawn 전에 실패시킨다"였다. 파일형 투영은
//! 원장도 영수증도 남기지 않으므로 영수증 부재가 더 이상 결격이 아니다 — 게이트는 옛
//! 영수증이 남아 있는 작업에 대해서만 의미를 갖는다.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db::{self, state};

#[tokio::test]
async fn memory_gate_lets_a_queued_task_without_a_receipt_through() {
    let suffix = std::process::id();
    let db_path = temp_root::dir()
        .join(format!("praxis-runner-queue-gate-{suffix}.sqlite"))
        .to_string_lossy()
        .into_owned();
    let worktree = temp_root::dir().join(format!("praxis-runner-queue-gate-{suffix}"));
    std::fs::create_dir_all(&worktree).unwrap();
    let pool = db::init_pool(&db_path).await.unwrap();
    praxis_lib::memory::migrate(&pool).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        "/tmp",
        "branch",
        "main",
        worktree.to_str().unwrap(),
        "must-not-spawn",
        Some("/bin/echo"),
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, state::QUEUED, 2)
        .await
        .unwrap();
    // 영수증이 없어도 시작 게이트는 통과한다 — 실을 것은 창고의 파일이지 DB 행이 아니다.
    assert!(praxis_lib::memory::verify_task_projection_for_start(&pool, task_id, 3)
        .await
        .unwrap()
        .is_none());
    assert_eq!(praxis_lib::memory::verify_task_projection(&pool, task_id, 3).await.unwrap(), 0);
    assert_eq!(
        db::get_task(&pool, task_id).await.unwrap().unwrap().state,
        state::QUEUED
    );
    let _ = std::fs::remove_dir_all(worktree);
    let _ = std::fs::remove_file(db_path);
}
