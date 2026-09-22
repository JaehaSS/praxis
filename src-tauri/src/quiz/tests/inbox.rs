//! inbox 수집 — 파일 격리, 중복 배제, 처리 흔적 보존.

use std::sync::atomic::{AtomicU32, Ordering};

use super::test_pool;
use crate::quiz::inbox::collect_and_store;

static DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

#[tokio::test]
async fn stores_valid_items_and_files_them_away() {
    let pool = test_pool().await;
    let dir = temp_dir();
    write(
        &dir,
        "a.json",
        r#"[{"kind":"vocab","question":"q1","answer":"a1"}]"#,
    );

    let stored = collect_and_store(&pool, &dir, 100).await.unwrap();

    assert_eq!(stored, 1);
    assert!(!dir.join("a.json").exists(), "처리한 파일이 그대로 남았다");
    assert!(dir.join("done/a.json").exists(), "done/으로 옮기지 않았다");
}

/// 한 파일이 깨져도 다른 파일은 들어가야 한다 — 배치가 통째로 날아가면 안 된다.
#[tokio::test]
async fn a_broken_file_does_not_block_the_others() {
    let pool = test_pool().await;
    let dir = temp_dir();
    write(&dir, "broken.json", "{{{");
    write(
        &dir,
        "good.json",
        r#"[{"kind":"trivia","question":"q","answer":"a"}]"#,
    );

    let stored = collect_and_store(&pool, &dir, 100).await.unwrap();

    assert_eq!(stored, 1);
}

#[tokio::test]
async fn duplicate_questions_are_stored_once() {
    let pool = test_pool().await;
    let dir = temp_dir();
    let body = r#"[{"kind":"vocab","question":"같은질문","answer":"a"}]"#;
    write(&dir, "first.json", body);
    assert_eq!(collect_and_store(&pool, &dir, 100).await.unwrap(), 1);

    write(&dir, "second.json", body);
    assert_eq!(
        collect_and_store(&pool, &dir, 200).await.unwrap(),
        0,
        "같은 질문이 두 번 들어갔다"
    );
}

/// 도메인 문제는 검수 대기로, 근거 본문은 그 시점에 복제되어야 한다.
#[tokio::test]
async fn domain_items_land_pending_with_their_excerpt() {
    let pool = test_pool().await;
    sqlx::query(
        "INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) \
         VALUES ('obsidian', 'n.md', 'document', '제목', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, content) \
         VALUES ((SELECT id FROM knowledge_nodes), 0, '롤백은 직전 태그로 되돌린다.')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (chunk_id,): (i64,) = sqlx::query_as("SELECT id FROM knowledge_chunks")
        .fetch_one(&pool)
        .await
        .unwrap();

    let dir = temp_dir();
    write(
        &dir,
        "d.json",
        &format!(
            r#"[{{"kind":"domain","question":"롤백 방법은?","answer":"직전 태그","chunk_id":{chunk_id}}}]"#
        ),
    );

    collect_and_store(&pool, &dir, 100).await.unwrap();

    let (status, excerpt): (String, Option<String>) =
        sqlx::query_as("SELECT status, source_excerpt FROM quiz_items")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "pending", "도메인 문제가 검수 없이 승인됐다");
    assert_eq!(excerpt.as_deref(), Some("롤백은 직전 태그로 되돌린다."));
}

/// inbox 디렉터리가 아직 없어도 터지지 않는다 — 생성이 한 번도 안 돌았을 때다.
#[tokio::test]
async fn a_missing_directory_is_not_an_error() {
    let pool = test_pool().await;
    let dir = crate::testtmp::dir().join("praxis-quiz-nonexistent-inbox");
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(collect_and_store(&pool, &dir, 100).await.unwrap(), 0);
}

fn temp_dir() -> std::path::PathBuf {
    let sequence = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = crate::testtmp::dir().join(format!(
        "praxis-quiz-inbox-{}-{sequence}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &std::path::Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).unwrap();
}
