//! 한 폴더를 여러 작업이 공유할 때의 투영 소유권.
//!
//! worktree를 쓰지 않으면 모든 작업이 프로젝트 루트의 같은 `AGENTS.md`를 본다. 그때 한 작업이
//! 메모리를 띄우면, 띄울 메모리가 없는 나머지 작업들이 그 블록을 자기 잔재로 읽어 재개할 때마다
//! 회수할 때마다 실패했다(`stale or damaged managed memory marker remains`). 반대로 그 나머지
//! 작업의 투영은 남의 블록을 지워 버렸다. 저널은 **자기가 쓴 바이트**에만 책임진다.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::{db, memory, projector};

static COUNTER: AtomicU32 = AtomicU32::new(0);

const NOW: i64 = 2_000_000_000;
const OWNER: &str = "# owner content\n";

struct Shared {
    pool: sqlx::SqlitePool,
    root: std::path::PathBuf,
    db_path: std::path::PathBuf,
}

impl Shared {
    fn claude_md(&self) -> String {
        std::fs::read_to_string(self.root.join("AGENTS.md")).unwrap()
    }
}

async fn shared() -> Shared {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = temp_root::dir().join(format!("praxis-shared-{}-{suffix}", std::process::id()));
    let db_path = base.with_extension("sqlite");
    let root = base.with_extension("worktree");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("AGENTS.md"), OWNER).unwrap();
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();

    // `/repo` 스코프에만 승인된 메모리를 둔다 — 다른 스코프의 작업은 띄울 것이 없다.
    let memory_id = memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::DECISION,
        "retain the owner file",
        Some("test"),
        NOW,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(&pool, memory_id, NOW + 1, None)
        .await
        .unwrap();
    memory::submit_for_review(&pool, memory_id, NOW + 2)
        .await
        .unwrap();
    memory::approve(&pool, memory_id, "human", NOW + 3)
        .await
        .unwrap();

    Shared {
        pool,
        root,
        db_path,
    }
}

/// 같은 루트를 쓰는 작업을 하나 만들고 투영까지 끝낸다. 반환값은 (task_id, 띄운 메모리 수).
async fn project(shared: &Shared, repo: &str, at: i64) -> (i64, usize) {
    let task_id = db::insert_task(
        &shared.pool,
        repo,
        "branch",
        "main",
        shared.root.to_str().unwrap(),
        "share the project root",
        Some("claude"),
        None,
        "terminal",
        at,
    )
    .await
    .unwrap();
    let count = memory::inject_into_worktree(
        &shared.pool,
        repo,
        "share the project root",
        None,
        task_id,
        at + 1,
        &shared.root,
        memory::INJECTION_LIMIT,
        &projector::project_targets(),
    )
    .await
    .unwrap();
    (task_id, count)
}

/// 재개는 기존 저널을 다시 검증한다(`existing_projection`) — 이 경로가 옆 작업의 블록에
/// 걸려 넘어지던 자리다.
async fn resume(shared: &Shared, task_id: i64, repo: &str, at: i64) -> anyhow::Result<usize> {
    memory::inject_into_worktree(
        &shared.pool,
        repo,
        "share the project root",
        None,
        task_id,
        at,
        &shared.root,
        memory::INJECTION_LIMIT,
        &projector::project_targets(),
    )
    .await
}

#[tokio::test]
async fn a_memoryless_projection_leaves_a_neighbours_block_alone() {
    let shared = shared().await;
    let (_, projected) = project(&shared, "/repo", NOW + 10).await;
    assert_eq!(projected, 1);
    let with_block = shared.claude_md();
    assert!(with_block.contains("PRAXIS MEMORY START"));

    let (_, none) = project(&shared, "/elsewhere", NOW + 20).await;

    assert_eq!(none, 0);
    assert_eq!(shared.claude_md(), with_block);
    cleanup(shared);
}

#[tokio::test]
async fn both_tasks_keep_verifying_while_they_share_the_file() {
    let shared = shared().await;
    let (owner, _) = project(&shared, "/repo", NOW + 10).await;
    let (guest, _) = project(&shared, "/elsewhere", NOW + 20).await;

    // 블록을 띄운 쪽도, 띄울 것이 없던 쪽도 재개할 수 있어야 한다.
    assert_eq!(resume(&shared, owner, "/repo", NOW + 30).await.unwrap(), 1);
    assert_eq!(
        resume(&shared, guest, "/elsewhere", NOW + 31)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        memory::verify_task_projection(&shared.pool, guest, NOW + 32)
            .await
            .unwrap(),
        0
    );
    cleanup(shared);
}

#[tokio::test]
async fn retiring_the_memoryless_task_neither_fails_nor_touches_the_block() {
    let shared = shared().await;
    let (owner, _) = project(&shared, "/repo", NOW + 10).await;
    let with_block = shared.claude_md();
    let (guest, _) = project(&shared, "/elsewhere", NOW + 20).await;

    memory::retire_task_projection(&shared.pool, guest, NOW + 30)
        .await
        .unwrap();

    assert_eq!(shared.claude_md(), with_block);
    let state: (String,) =
        sqlx::query_as("SELECT state FROM memory_projection_journal WHERE task_id = ?")
            .bind(guest)
            .fetch_one(&shared.pool)
            .await
            .unwrap();
    assert_eq!(state.0, "retired");

    // 이웃이 물러난 뒤에도 소유자의 회수는 자기 바이트를 정확히 걷어 간다.
    memory::retire_task_projection(&shared.pool, owner, NOW + 31)
        .await
        .unwrap();
    assert_eq!(shared.claude_md(), OWNER);
    cleanup(shared);
}

/// 소유자가 먼저 물러나도 남는 것은 사용자 파일뿐이고, 뒤늦은 이웃의 회수가 그것을 지우지
/// 않는다 — 이웃의 preimage에는 블록이 들어 있으므로 되돌리면 지운 블록이 되살아난다.
#[tokio::test]
async fn the_guest_does_not_resurrect_a_block_the_owner_already_retired() {
    let shared = shared().await;
    let (owner, _) = project(&shared, "/repo", NOW + 10).await;
    let (guest, _) = project(&shared, "/elsewhere", NOW + 20).await;

    memory::retire_task_projection(&shared.pool, owner, NOW + 30)
        .await
        .unwrap();
    assert_eq!(shared.claude_md(), OWNER);

    memory::retire_task_projection(&shared.pool, guest, NOW + 31)
        .await
        .unwrap();

    assert_eq!(shared.claude_md(), OWNER);
    cleanup(shared);
}

fn cleanup(shared: Shared) {
    let _ = std::fs::remove_dir_all(shared.root);
    let _ = std::fs::remove_file(shared.db_path);
}
