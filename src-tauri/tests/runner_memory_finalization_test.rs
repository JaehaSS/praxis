//! Runner approval must take the memory block back out before merge.
//!
//! 파일형 메모리(설계 2026-09-13)에서 블록은 창고 파일의 세션용 사본이다 — 승인 커밋에
//! 섞이면 저장소의 `AGENTS.md`에 영구히 들어간다. 마커 밖 사용자·에이전트 내용은 보존한다.

#[path = "support/temp_root.rs"]
mod temp_root;

#[path = "support/runner_memory_finalization.rs"]
mod support;

use praxis_lib::db;
use praxis_lib::runner;
use support::{cleanup, fixture};

#[tokio::test]
async fn approval_does_not_merge_the_memory_block_into_tracked_context() {
    let fixture = fixture("non-leak").await;
    let projected =
        std::fs::read_to_string(std::path::Path::new(&fixture.task.worktree_path).join("AGENTS.md"))
            .unwrap();
    assert!(projected.contains("runner finalize invariant"), "{projected}");
    std::fs::write(
        std::path::Path::new(&fixture.task.worktree_path).join("note.txt"),
        "approved change\n",
    )
    .unwrap();
    runner::finalize_task(
        &fixture.config,
        &fixture.pool,
        &fixture.locks,
        fixture.task.id,
        true,
        2_000_000_010,
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("AGENTS.md")).unwrap(),
        "# owner\n"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("note.txt")).unwrap(),
        "approved change\n"
    );
    cleanup(fixture);
}

#[tokio::test]
async fn approval_keeps_agent_edits_to_a_projection_target() {
    let fixture = fixture("tamper").await;
    let path = std::path::Path::new(&fixture.task.worktree_path).join("AGENTS.md");
    let mut changed = std::fs::read_to_string(&path).unwrap();
    changed.push_str("\nagent-owned edit\n");
    std::fs::write(&path, &changed).unwrap();
    runner::finalize_task(
        &fixture.config,
        &fixture.pool,
        &fixture.locks,
        fixture.task.id,
        true,
        2_000_000_010,
    )
    .await
    .unwrap();
    // 마커 밖 편집은 살아남고 블록만 사라진다.
    let merged = std::fs::read_to_string(fixture.repo.join("AGENTS.md")).unwrap();
    assert!(merged.contains("# owner"), "{merged}");
    assert!(merged.contains("agent-owned edit"), "{merged}");
    assert!(!merged.contains("PRAXIS MEMORY"), "{merged}");
    assert_eq!(
        db::get_task(&fixture.pool, fixture.task.id)
            .await
            .unwrap()
            .unwrap()
            .state,
        db::state::DONE
    );
    cleanup(fixture);
}

#[tokio::test]
async fn discard_removes_the_worktree_that_carried_the_block() {
    let fixture = fixture("discard-retire").await;
    runner::finalize_task(
        &fixture.config,
        &fixture.pool,
        &fixture.locks,
        fixture.task.id,
        false,
        2_000_000_010,
    )
    .await
    .unwrap();
    assert!(!std::path::Path::new(&fixture.task.worktree_path).exists());
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("AGENTS.md")).unwrap(),
        "# owner\n"
    );
    cleanup(fixture);
}
