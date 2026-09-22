#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

#[cfg(unix)]
#[tokio::test]
async fn context_reader_rejects_symlinks_and_paths_outside_the_report() {
    use std::os::unix::fs::symlink;

    let root = temp_root::dir().join(format!("praxis-context-audit-{}", std::process::id()));
    let home = root.join("home");
    let worktree = root.join("worktree");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&worktree).unwrap();
    let secret = root.join("secret.txt");
    std::fs::write(&secret, "must not leak").unwrap();
    symlink(&secret, worktree.join("CLAUDE.md")).unwrap();
    let db_path = root.join("audit.sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        worktree.to_str().unwrap(),
        "audit",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();

    assert!(
        memory::context_audit::read_file(&pool, task_id, &home, &worktree.join("CLAUDE.md"),)
            .await
            .is_err()
    );
    assert!(
        memory::context_audit::read_file(&pool, task_id, &home, &secret)
            .await
            .is_err()
    );
    let report = memory::context_audit::report(&pool, task_id, &home, false, false)
        .await
        .unwrap();
    assert!(!report.vendors[0].files[1].has_praxis_block);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn report_separates_immutable_selection_from_current_scope_readiness() {
    let root = temp_root::dir().join(format!("praxis-context-summary-{}", std::process::id()));
    let home = root.join("home");
    let worktree = root.join("worktree");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&worktree).unwrap();
    let db_path = root.join("summary.sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let now = 2_000_000_000;
    memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::CLAIM,
        "review this current candidate",
        Some("test"),
        now,
    )
    .await
    .unwrap();
    let verified_id = memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::DECISION,
        "stable memory phrase",
        Some("test"),
        now,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(&pool, verified_id, now, None)
        .await
        .unwrap();
    memory::submit_for_review(&pool, verified_id, now)
        .await
        .unwrap();
    memory::approve(&pool, verified_id, "human", now)
        .await
        .unwrap();
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        worktree.to_str().unwrap(),
        "unrelated query words",
        None,
        None,
        "terminal",
        now,
    )
    .await
    .unwrap();
    memory::inject_into_worktree(
        &pool,
        "/repo",
        "unrelated query words",
        None,
        task_id,
        now,
        &worktree,
        memory::INJECTION_LIMIT,
        &["CLAUDE.md"],
    )
    .await
    .unwrap();

    let report = memory::context_audit::report(&pool, task_id, &home, false, false)
        .await
        .unwrap();
    assert_eq!(report.projection.as_ref().unwrap().state, "applied");
    assert_eq!(report.projection.as_ref().unwrap().selected_count, 0);
    assert_eq!(report.memory_counts.scope_total, 2);
    assert_eq!(report.memory_counts.actionable, 1);
    assert_eq!(report.memory_counts.verified, 1);
    assert_eq!(report.memory_counts.eligible, 1);

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn context_reader_rejects_fifo_without_blocking() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::time::Duration;

    let root = temp_root::dir().join(format!("praxis-context-fifo-{}", std::process::id()));
    let home = root.join("home");
    let worktree = root.join("worktree");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&worktree).unwrap();
    let fifo = worktree.join("CLAUDE.md");
    let raw = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { nix::libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
    let db_path = root.join("audit.sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        worktree.to_str().unwrap(),
        "audit",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(memory::context_audit::read_file(
            &pool, task_id, &home, &fifo,
        ))
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
    assert!(!blocked, "context FIFO blocked before type validation");
    assert!(result.is_err());
    let _ = std::fs::remove_dir_all(root);
}
