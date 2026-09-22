//! Evidence path confinement and append-only identity constraints.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, evidence, memory};

#[tokio::test]
async fn local_document_evidence_rejects_symlinks_and_oversized_sources() {
    let (pool, root, db_path, memory_id) = fixture("source-policy").await;
    let outside = root.with_extension("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, root.join("linked.txt")).unwrap();
        assert!(add_document(&pool, memory_id, "linked.txt").await.is_err());
    }
    std::fs::write(root.join("large.txt"), vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
    assert!(add_document(&pool, memory_id, "large.txt").await.is_err());
    cleanup(root, db_path, outside);
}

#[tokio::test]
async fn evidence_identity_and_check_receipts_are_append_only() {
    let (pool, root, db_path, memory_id) = fixture("immutability").await;
    std::fs::write(root.join("guide.md"), "guide\n").unwrap();
    let evidence_id = add_document(&pool, memory_id, "guide.md").await.unwrap();
    evidence::revalidate_memory(&pool, memory_id, 101)
        .await
        .unwrap();
    let check_id: i64 = sqlx::query_scalar(
        "SELECT id FROM memory_evidence_checks WHERE evidence_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(evidence_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        sqlx::query("UPDATE memory_evidence SET locator_json = '{}' WHERE id = ?")
            .bind(evidence_id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("DELETE FROM memory_evidence WHERE id = ?")
        .bind(evidence_id)
        .execute(&pool)
        .await
        .is_err());
    assert!(
        sqlx::query("UPDATE memory_evidence_checks SET status = 'changed' WHERE id = ?")
            .bind(check_id)
            .execute(&pool)
            .await
            .is_err()
    );
    let unused = root.with_extension("unused");
    cleanup(root, db_path, unused);
}

#[cfg(unix)]
#[tokio::test]
async fn local_document_rejects_fifo_without_blocking_on_open() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::FileTypeExt;
    use std::time::Duration;

    let (pool, root, db_path, memory_id) = fixture("fifo-policy").await;
    let fifo = root.join("source.pipe");
    let raw = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { nix::libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(add_document(&pool, memory_id, "source.pipe"))
    });

    std::thread::sleep(Duration::from_millis(250));
    let blocked = !worker.is_finished();
    if blocked {
        let fd = unsafe {
            nix::libc::open(
                raw.as_ptr(),
                nix::libc::O_WRONLY | nix::libc::O_NONBLOCK | nix::libc::O_CLOEXEC,
            )
        };
        if fd >= 0 {
            unsafe { nix::libc::close(fd) };
        }
    }
    let result = worker.join().unwrap();
    assert!(!blocked, "FIFO open blocked before source type validation");
    assert!(result.is_err());
    assert!(std::fs::metadata(&fifo).unwrap().file_type().is_fifo());
    let unused = root.with_extension("unused");
    cleanup(root, db_path, unused);
}

async fn fixture(
    label: &str,
) -> (
    sqlx::SqlitePool,
    std::path::PathBuf,
    std::path::PathBuf,
    i64,
) {
    let root = temp_root::dir().join(format!(
        "praxis-evidence-security-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let db_path = root.with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let memory_id = memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some(root.to_string_lossy().as_ref()),
        memory::knowledge_type::CLAIM,
        "secured document claim",
        Some("test"),
        99,
    )
    .await
    .unwrap();
    (pool, root, db_path, memory_id)
}

async fn add_document(
    pool: &sqlx::SqlitePool,
    memory_id: i64,
    relative_path: &str,
) -> anyhow::Result<i64> {
    evidence::add_local_document(
        pool,
        memory_id,
        evidence::LocalDocumentInput {
            relative_path: relative_path.into(),
            expires_at: None,
        },
        100,
    )
    .await
}

fn cleanup(root: std::path::PathBuf, db_path: std::path::PathBuf, outside: std::path::PathBuf) {
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(outside);
    let _ = std::fs::remove_dir_all(root);
}
