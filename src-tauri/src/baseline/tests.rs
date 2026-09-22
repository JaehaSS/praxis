use std::sync::atomic::{AtomicU32, Ordering};

use super::pin_missing_baselines;
use crate::{db, worktree};

static COUNTER: AtomicU32 = AtomicU32::new(0);

async fn test_pool(tag: &str) -> sqlx::SqlitePool {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "praxis-baseline-{tag}-{}-{sequence}.sqlite",
        std::process::id()
    ));
    // WAL 모드라 `-wal`·`-shm`까지 지운다 — 본체만 지우면 WAL이 옛 페이지를 되살린다.
    for suffix in ["", "-wal", "-shm"] {
        std::fs::remove_file(format!("{}{suffix}", path.display())).ok();
    }
    db::init_pool(path.to_str().unwrap()).await.unwrap()
}

fn temp_repo(tag: &str) -> std::path::PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "praxis-baseline-repo-{tag}-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("seed.txt"), "seed\n").unwrap();
    worktree::init_repository(&dir).unwrap();
    dir
}

/// 굳히기는 한 번만 일어나야 한다. 두 번째 실행이 값을 다시 계산하면 그 사이 base가
/// 움직였을 때 기준점이 밀려, 불변으로 두려던 이유가 무너진다.
#[tokio::test]
async fn pinning_twice_changes_nothing() {
    let pool = test_pool("idempotent").await;
    let repo = temp_repo("idempotent");
    let wt = worktree::create_plain(&repo, "praxis/backfill", None).unwrap();
    let id = db::insert_task(
        &pool,
        repo.to_str().unwrap(),
        &wt.branch,
        &wt.base,
        wt.path.to_str().unwrap(),
        "backfill 대상",
        None,
        None,
        "terminal",
        0,
    )
    .await
    .unwrap();

    assert_eq!(
        pin_missing_baselines(&pool).await.unwrap(),
        1,
        "기준점이 없는 진행 중 작업은 굳혀야 한다"
    );
    let pinned = db::get_task(&pool, id)
        .await
        .unwrap()
        .unwrap()
        .base_revision
        .expect("기준점이 기록되지 않았다");

    assert_eq!(
        pin_missing_baselines(&pool).await.unwrap(),
        0,
        "두 번째 실행은 아무것도 굳히지 않는다"
    );
    assert_eq!(
        db::get_task(&pool, id).await.unwrap().unwrap().base_revision,
        Some(pinned),
        "이미 굳은 값은 다시 계산되지 않는다"
    );

    std::fs::remove_dir_all(&repo).ok();
}

/// 워크트리가 사라진 작업에서 멈추면 그 뒤 작업들이 통째로 굳지 못한다.
#[tokio::test]
async fn a_missing_worktree_does_not_stop_the_rest() {
    let pool = test_pool("resilient").await;
    let repo = temp_repo("resilient");
    db::insert_task(
        &pool,
        "/nowhere",
        "praxis/gone",
        "main",
        "/nowhere/worktree",
        "워크트리가 없는 작업",
        None,
        None,
        "terminal",
        0,
    )
    .await
    .unwrap();
    let wt = worktree::create_plain(&repo, "praxis/alive", None).unwrap();
    let alive = db::insert_task(
        &pool,
        repo.to_str().unwrap(),
        &wt.branch,
        &wt.base,
        wt.path.to_str().unwrap(),
        "살아 있는 작업",
        None,
        None,
        "terminal",
        0,
    )
    .await
    .unwrap();

    assert_eq!(pin_missing_baselines(&pool).await.unwrap(), 1);
    assert!(
        db::get_task(&pool, alive)
            .await
            .unwrap()
            .unwrap()
            .base_revision
            .is_some(),
        "앞선 작업의 실패가 뒤를 막아서는 안 된다"
    );

    std::fs::remove_dir_all(&repo).ok();
}
