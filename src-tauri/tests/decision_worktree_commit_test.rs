#[path = "support/temp_root.rs"]
mod temp_root;

use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::worktree;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn repo() -> std::path::PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = temp_root::dir().join(format!(
        "praxis-decision-worktree-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "decision@example.test"]);
    git(&root, &["config", "user.name", "Decision Test"]);
    std::fs::write(root.join("README.md"), "before\n").unwrap();
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-qm", "initial"]);
    root
}

#[test]
fn generated_mcp_exclusion_is_available_as_an_idempotent_commit_stage() {
    let root = repo();
    let worktree = worktree::create_plain(&root, "praxis/decision-policy", None).unwrap();
    std::fs::write(worktree.path.join(".mcp.json"), "PRIVATE_MCP\n").unwrap();
    std::fs::write(worktree.path.join("result.txt"), "approved\n").unwrap();

    let commit = worktree
        .commit_for_approval_with_generated_mcp_excluded()
        .unwrap();
    let retried = worktree
        .commit_for_approval_with_generated_mcp_excluded()
        .unwrap();
    assert_eq!(commit, retried);
    assert!(worktree.path.exists());
    assert!(!root.join("result.txt").exists());
    let committed = git(
        &worktree.path,
        &["show", "--format=", "--name-only", &commit],
    );
    assert!(committed.contains("result.txt"));
    assert!(!committed.contains(".mcp.json"));

    worktree.merge_for_approval(&commit).unwrap();
    worktree.cleanup_after_finalization().unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("result.txt")).unwrap(),
        "approved\n"
    );
    assert!(!root.join(".mcp.json").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn generated_mcp_is_unstaged_before_the_decision_commit() {
    let root = repo();
    let worktree = worktree::create_plain(&root, "praxis/staged-mcp", None).unwrap();
    std::fs::write(worktree.path.join(".mcp.json"), "PRIVATE_STAGED_MCP\n").unwrap();
    std::fs::write(worktree.path.join("result.txt"), "approved\n").unwrap();
    git(&worktree.path, &["add", ".mcp.json"]);

    let commit = worktree
        .commit_for_approval_with_generated_mcp_excluded()
        .unwrap();

    let committed = git(
        &worktree.path,
        &["show", "--format=", "--name-only", &commit],
    );
    assert!(committed.contains("result.txt"));
    assert!(!committed.contains(".mcp.json"));
    worktree.cleanup_after_finalization().unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn generated_mcp_in_existing_commit_history_fails_closed() {
    let root = repo();
    let worktree = worktree::create_plain(&root, "praxis/committed-mcp", None).unwrap();
    std::fs::write(worktree.path.join(".mcp.json"), "PRIVATE_COMMITTED_MCP\n").unwrap();
    std::fs::write(worktree.path.join("result.txt"), "approved\n").unwrap();
    git(&worktree.path, &["add", "-A"]);
    git(&worktree.path, &["commit", "-qm", "agent commit"]);

    let error = worktree
        .commit_for_approval_with_generated_mcp_excluded()
        .unwrap_err();

    assert!(error.to_string().contains("approval commit history"));
    assert!(!root.join("result.txt").exists());
    worktree.cleanup_after_finalization().unwrap();
    let _ = std::fs::remove_dir_all(root);
}
