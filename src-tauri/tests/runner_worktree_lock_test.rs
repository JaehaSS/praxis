#[path = "support/temp_root.rs"]
mod temp_root;

use std::time::Duration;

use praxis_lib::runner::worktree_lock::WorktreeLocks;

#[tokio::test]
async fn same_worktree_mutations_are_exclusive() {
    let locks = WorktreeLocks::default();
    let path = temp_root::dir().join("praxis-runner-lock-test");
    let first = locks.acquire(&path).await;
    let contender = {
        let locks = locks.clone();
        let path = path.clone();
        tokio::spawn(async move { locks.acquire(&path).await })
    };

    assert!(tokio::time::timeout(Duration::from_millis(20), contender)
        .await
        .is_err());
    drop(first);
    let second = tokio::time::timeout(Duration::from_secs(1), locks.acquire(&path))
        .await
        .unwrap();
    drop(second);
}
