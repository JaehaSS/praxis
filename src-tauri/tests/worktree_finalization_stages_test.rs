#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::worktree;

static COUNTER: AtomicU32 = AtomicU32::new(0);

#[test]
fn approval_stages_are_idempotent_for_crash_recovery() {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let repo = temp_root::dir().join(format!(
        "praxis-worktree-finalize-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "runner@example.test"]);
    git(&repo, &["config", "user.name", "Runner Test"]);
    std::fs::write(repo.join("README.md"), "before\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "initial"]);
    let worktree = worktree::create_plain(&repo, "praxis/finalize-stages", None).unwrap();
    std::fs::write(worktree.path.join("README.md"), "after\n").unwrap();

    let commit = worktree.commit_for_approval().unwrap();
    worktree.merge_for_approval(&commit).unwrap();
    worktree.merge_for_approval(&commit).unwrap();
    assert!(worktree.commit_is_merged(&commit));
    worktree.cleanup_after_finalization().unwrap();
    worktree.cleanup_after_finalization().unwrap();

    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "after\n"
    );
    assert!(!worktree.path.exists());
    let _ = std::fs::remove_dir_all(repo);
}

fn git(root: &std::path::Path, args: &[&str]) {
    assert!(std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap()
        .success());
}
