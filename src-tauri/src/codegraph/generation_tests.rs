use super::test_support::{count, test_pool};
use super::{generation, snapshot};

#[tokio::test]
async fn migration_creates_generation_tables_idempotently() {
    let pool = test_pool("migration").await;
    super::migrate(&pool).await.unwrap();
    let (tables,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN \
         ('code_graph_runs','code_graph_active','code_graph_files',\
          'code_graph_nodes','code_graph_edges')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tables, 5);
}

#[tokio::test]
async fn failed_generation_keeps_the_last_ready_generation_active() {
    let pool = test_pool("failure").await;
    let first = generation::start_run(&pool, "/w", "fingerprint-1", 10)
        .await
        .unwrap();
    generation::promote(&pool, first, generation::RunStats::default(), 20)
        .await
        .unwrap();
    let second = generation::start_run(&pool, "/w", "fingerprint-2", 30)
        .await
        .unwrap();
    generation::finish_failed(&pool, second, "references 실패", 40)
        .await
        .unwrap();

    assert_eq!(
        generation::active_run_id(&pool, "/w").await.unwrap(),
        Some(first)
    );
    assert_eq!(
        generation::run_state(&pool, second).await.unwrap(),
        "degraded"
    );
}

#[tokio::test]
async fn promoting_replaces_the_pointer_and_run_delete_cascades() {
    let pool = test_pool("cascade").await;
    let run = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let file = snapshot::insert_file(&pool, run, "src/lib.rs", "hash", "rust", None, None)
        .await
        .unwrap();
    snapshot::insert_test_node(&pool, run, file, "target")
        .await
        .unwrap();
    generation::promote(&pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();

    sqlx::query("DELETE FROM code_graph_runs WHERE id = ?")
        .bind(run)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(count(&pool, "code_graph_files").await, 0);
    assert_eq!(count(&pool, "code_graph_nodes").await, 0);
    assert_eq!(generation::active_run_id(&pool, "/w").await.unwrap(), None);
}

#[tokio::test]
async fn a_third_success_keeps_only_one_rollback_generation() {
    let pool = test_pool("prune").await;
    for n in 1..=3 {
        let run = generation::start_run(&pool, "/w", &format!("fingerprint-{n}"), n * 10)
            .await
            .unwrap();
        generation::promote(&pool, run, generation::RunStats::default(), n * 10 + 1)
            .await
            .unwrap();
    }
    assert_eq!(count(&pool, "code_graph_runs").await, 2);
}

#[tokio::test]
async fn purge_removes_generation_rows_without_changing_legacy_count_contract() {
    let pool = test_pool("purge").await;
    let run = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    generation::promote(&pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();

    assert_eq!(super::purge_worktree(&pool, "/w").await.unwrap(), 0);
    assert_eq!(count(&pool, "code_graph_runs").await, 0);
}
