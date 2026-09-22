use std::io::{Error, ErrorKind};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn second_target_failure_restores_every_preimage() {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let worktree = crate::testtmp::dir().join(format!(
        "praxis-projector-rollback-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join("CLAUDE.md"), "owner-a").unwrap();
    std::fs::write(worktree.join("AGENTS.md"), "owner-b").unwrap();
    let preimages = super::capture_targets(&worktree, &["CLAUDE.md", "AGENTS.md"]).unwrap();
    let updates = vec![Some("managed-a".to_string()), Some("managed-b".to_string())];
    let mut calls = 0;

    let result =
        super::apply_updates_with(&worktree, &preimages, &updates, |path, preimage, update| {
            calls += 1;
            if calls == 2 {
                return Err(Error::other("injected second write failure"));
            }
            super::io::write_update(path, preimage, update)
        });

    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(worktree.join("CLAUDE.md")).unwrap(),
        "owner-a"
    );
    assert_eq!(
        std::fs::read_to_string(worktree.join("AGENTS.md")).unwrap(),
        "owner-b"
    );
    let _ = std::fs::remove_dir_all(worktree);
}

#[test]
fn rollback_refuses_to_overwrite_an_independent_edit() {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let worktree =
        crate::testtmp::dir().join(format!("praxis-projector-cas-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join("CLAUDE.md"), "owner").unwrap();
    let preimages = super::capture_targets(&worktree, &["CLAUDE.md"]).unwrap();
    let updates = vec![Some("managed".to_string())];
    std::fs::write(worktree.join("CLAUDE.md"), "new owner edit").unwrap();

    let result = super::restore_matching(&worktree, &preimages, &updates);

    assert_eq!(result.unwrap_err().kind(), ErrorKind::WouldBlock);
    assert_eq!(
        std::fs::read_to_string(worktree.join("CLAUDE.md")).unwrap(),
        "new owner edit"
    );
    let _ = std::fs::remove_dir_all(worktree);
}

#[test]
fn rollback_removes_a_newly_created_projection_target() {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let worktree = crate::testtmp::dir().join(format!(
        "praxis-projector-new-target-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&worktree).unwrap();
    let preimages = super::capture_targets(&worktree, &["CLAUDE.md"]).unwrap();
    let updates = vec![Some("managed".to_string())];
    super::apply_updates(&worktree, &preimages, &updates).unwrap();

    super::restore_matching(&worktree, &preimages, &updates).unwrap();

    assert!(!worktree.join("CLAUDE.md").exists());
    let _ = std::fs::remove_dir_all(worktree);
}

#[cfg(unix)]
#[test]
fn projection_preserves_private_file_mode() {
    use std::os::unix::fs::PermissionsExt;

    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let worktree =
        crate::testtmp::dir().join(format!("praxis-projector-mode-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&worktree).unwrap();
    let target = worktree.join("CLAUDE.md");
    std::fs::write(&target, "owner").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    let preimages = super::capture_targets(&worktree, &["CLAUDE.md"]).unwrap();

    super::apply_updates(&worktree, &preimages, &[Some("managed".to_string())]).unwrap();

    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let _ = std::fs::remove_dir_all(worktree);
}
