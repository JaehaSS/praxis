use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::knowledge::vault::{index_revision, migrate, register_vault, search_browse};

const FILES: usize = 10_000;
const TOTAL_BYTES: usize = 100 * 1024 * 1024;

#[tokio::test]
#[ignore = "run the explicit release vault performance gate"]
async fn ten_thousand_file_index_has_sub_two_second_warm_p95() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let database = database_path(&pool).await;
    let root = root();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let revisions = seed(&pool, &root, &vault.id).await;
    let cold_start = Instant::now();
    for revision in &revisions {
        index_revision(&pool, revision).await.unwrap();
    }
    let cold = cold_start.elapsed();
    for query in ["needle", "seeded", "내용", "file 0042", "needle"] {
        search_browse(&pool, query, 0).await.unwrap();
    }
    let mut samples = Vec::new();
    for index in 0..30 {
        let query = ["needle", "seeded", "내용", "file 0042", "needle"][index % 5];
        let start = Instant::now();
        let page = search_browse(&pool, query, 0).await.unwrap();
        assert!(!page.hits.is_empty());
        samples.push(start.elapsed());
    }
    samples.sort_unstable();
    let p95 = samples[(samples.len() * 95).div_ceil(100) - 1];
    println!(
        "vault performance: db=file:{database}, files={FILES}, bytes={TOTAL_BYTES}, seed=fixed-v1, cold_index_ms={}, warm_samples={}, warm_p95_ms={}, env={}/{}",
        cold.as_millis(), samples.len(), p95.as_millis(), std::env::consts::OS, std::env::consts::ARCH,
    );
    assert!(p95 <= Duration::from_secs(2));
    std::fs::remove_dir_all(root).unwrap();
}

async fn database_path(pool: &SqlitePool) -> String {
    let path: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(!path.is_empty());
    path
}

fn root() -> PathBuf {
    let root = crate::testtmp::dir().join(format!("vault-performance-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("bulk")).unwrap();
    root
}

async fn seed(pool: &SqlitePool, root: &Path, vault_id: &str) -> Vec<String> {
    let mut tx = pool.begin().await.unwrap();
    let mut revisions = Vec::with_capacity(FILES);
    for index in 0..FILES {
        let document_id = format!("perf-document-{index:05}");
        let revision_id = format!("perf-revision-{index:05}");
        let path = format!("bulk/{index:05}.txt");
        let content = content(index);
        std::fs::write(root.join(&path), &content).unwrap();
        let hash = format!("{:x}", Sha256::digest(&content));
        sqlx::query("INSERT INTO vault_documents (id, vault_id, kind, title, state, created_at) VALUES (?, ?, 'source', ?, 'active', 1)")
            .bind(&document_id)
            .bind(vault_id)
            .bind(format!("seeded file {index:05}"))
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO vault_revisions (id, document_id, relative_path, sha256, size, created_at) VALUES (?, ?, ?, ?, ?, 1)")
            .bind(&revision_id)
            .bind(&document_id)
            .bind(path)
            .bind(hash)
            .bind(content.len() as i64)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE vault_documents SET current_revision = ? WHERE id = ?")
            .bind(&revision_id)
            .bind(document_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        revisions.push(revision_id);
    }
    tx.commit().await.unwrap();
    revisions
}

fn content(index: usize) -> Vec<u8> {
    let size = TOTAL_BYTES / FILES + usize::from(index < TOTAL_BYTES % FILES);
    let mut content = format!("seeded needle 내용 file {index:05}\n").into_bytes();
    content.resize(size, b'x');
    content
}
