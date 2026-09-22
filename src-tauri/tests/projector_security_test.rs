//! Hostile projection-target coverage.

#[path = "support/temp_root.rs"]
mod temp_root;

#[cfg(unix)]
#[test]
fn projector_rejects_symlink_targets_without_touching_external_file() {
    use std::os::unix::fs::symlink;

    let nonce = format!("{}", std::process::id());
    let worktree = temp_root::dir().join(format!("praxis-symlink-wt-{nonce}"));
    let external = temp_root::dir().join(format!("praxis-symlink-external-{nonce}"));
    let _ = std::fs::remove_dir_all(&worktree);
    let _ = std::fs::remove_file(&external);
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(&external, "external owner content\n").unwrap();
    symlink(&external, worktree.join("CLAUDE.md")).unwrap();

    let result = praxis_lib::projector::write_block(
        &worktree,
        &["CLAUDE.md"],
        "<!-- START -->",
        "<!-- END -->",
        "<!-- START -->\nmanaged\n<!-- END -->",
    );
    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(&external).unwrap(),
        "external owner content\n"
    );
    let _ = std::fs::remove_dir_all(worktree);
    let _ = std::fs::remove_file(external);
}

#[test]
fn projector_rejects_invalid_utf8_without_replacing_target() {
    let worktree =
        temp_root::dir().join(format!("praxis-invalid-utf8-wt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&worktree);
    std::fs::create_dir_all(&worktree).unwrap();
    let original = vec![0xff, 0xfe, 0xfd];
    std::fs::write(worktree.join("CLAUDE.md"), &original).unwrap();

    let result = praxis_lib::projector::write_block(
        &worktree,
        &["CLAUDE.md"],
        "<!-- START -->",
        "<!-- END -->",
        "<!-- START -->\nmanaged\n<!-- END -->",
    );

    assert!(result.is_err());
    assert_eq!(std::fs::read(worktree.join("CLAUDE.md")).unwrap(), original);
    let _ = std::fs::remove_dir_all(worktree);
}

#[test]
fn projector_rejects_oversized_target_without_replacing_it() {
    let worktree = temp_root::dir().join(format!("praxis-oversized-wt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&worktree);
    std::fs::create_dir_all(&worktree).unwrap();
    let original = vec![b'x'; 4 * 1024 * 1024 + 1];
    std::fs::write(worktree.join("CLAUDE.md"), &original).unwrap();

    let result = praxis_lib::projector::write_block(
        &worktree,
        &["CLAUDE.md"],
        "<!-- START -->",
        "<!-- END -->",
        "<!-- START -->\nmanaged\n<!-- END -->",
    );

    assert!(result.is_err());
    assert_eq!(
        std::fs::metadata(worktree.join("CLAUDE.md")).unwrap().len(),
        original.len() as u64
    );
    let _ = std::fs::remove_dir_all(worktree);
}

#[test]
fn projector_rejects_directory_target() {
    let worktree =
        temp_root::dir().join(format!("praxis-directory-target-wt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&worktree);
    std::fs::create_dir_all(worktree.join("CLAUDE.md")).unwrap();

    let result = praxis_lib::projector::write_block(
        &worktree,
        &["CLAUDE.md"],
        "<!-- START -->",
        "<!-- END -->",
        "<!-- START -->\nmanaged\n<!-- END -->",
    );

    assert!(result.is_err());
    assert!(worktree.join("CLAUDE.md").is_dir());
    let _ = std::fs::remove_dir_all(worktree);
}
