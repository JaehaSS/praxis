//! 파일 브라우저 조작의 워크트리 가드 — `fsapi::guard` 계층에서 검증한다.
//!
//! 커맨드 자체는 `State<AppState>`를 요구해 통합 테스트가 무거우므로, 판정 로직을
//! 직접 친다. 커맨드는 이 함수들을 통과하는 것 외에 다른 경로가 없다
//! (`commands.rs`의 `assert_paths_mutable`).

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db;
use praxis_lib::fsapi::guard::{assert_owners_mutable, owner_tasks};

/// 병렬 테스트 간 충돌을 막는 유일 임시 루트 — 이 저장소는 tempfile을 쓰지 않는다.
fn tmp_root(label: &str) -> std::path::PathBuf {
    let dir = temp_root::dir().join(format!(
        "praxis-fs-guard-{}-{}-{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn guard_blocks_inside_worktree_and_respects_path_boundaries() {
    let base = tmp_root("base");
    // `/…/work`와 `/…/workspace` — 문자열 prefix로 비교하면 후자가 전자에 잘못 걸린다.
    let work = base.join("work");
    let workspace = base.join("workspace");
    std::fs::create_dir_all(work.join("src")).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(work.join("src/a.rs"), "x").unwrap();
    std::fs::write(workspace.join("b.rs"), "y").unwrap();

    let db_path = base.join("guard.sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        work.to_str().unwrap(),
        "guard",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();

    // 1. 워크트리 하위 파일은 그 작업이 소유한다.
    let inside = work.join("src/a.rs").canonicalize().unwrap();
    let owners = owner_tasks(&pool, &inside).await.unwrap();
    assert_eq!(owners.len(), 1, "워크트리 하위 파일은 소유 작업이 있어야 한다");
    assert_eq!(owners[0].id, task_id);

    // 2. 워크트리 자기 자신도 소유 대상이다.
    let root_itself = work.canonicalize().unwrap();
    assert_eq!(owner_tasks(&pool, &root_itself).await.unwrap().len(), 1);

    // 3. 경계 — `workspace`는 `work` 작업에 걸리지 않는다.
    let sibling = workspace.join("b.rs").canonicalize().unwrap();
    assert!(
        owner_tasks(&pool, &sibling).await.unwrap().is_empty(),
        "work가 workspace를 삼키면 안 된다"
    );

    // 4. Running 상태에서는 변경이 허용된다.
    let owners = owner_tasks(&pool, &inside).await.unwrap();
    assert!(assert_owners_mutable(&pool, &owners).await.is_ok());

    // 5. Finalizing 상태가 되면 거부한다.
    db::update_state(&pool, task_id, db::state::FINALIZING, 2)
        .await
        .unwrap();
    let owners = owner_tasks(&pool, &inside).await.unwrap();
    assert!(
        assert_owners_mutable(&pool, &owners).await.is_err(),
        "Finalizing 작업의 워크트리는 변경할 수 없다"
    );

    let _ = std::fs::remove_dir_all(&base);
}
