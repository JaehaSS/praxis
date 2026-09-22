#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{
    approval, db,
    worktree::{self, Worktree},
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU32, Ordering},
};

static SERIAL: AtomicU32 = AtomicU32::new(0);
struct Fixture {
    root: PathBuf,
    task: Worktree,
}
impl Fixture {
    fn new() -> Self {
        let root = temp_root::dir().join(format!(
            "praxis-readiness-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "dev"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        git(&root, &["config", "user.name", "Approval Test"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(&root, &["config", "core.hooksPath", ".git/hooks"]);
        std::fs::write(root.join("base.txt"), "base\n").unwrap();
        git(&root, &["add", "base.txt"]);
        git(&root, &["commit", "-qm", "base"]);
        let task = worktree::create_plain(&root, "praxis/inspect", Some("dev")).unwrap();
        Self { root, task }
    }
    fn commit_task(&self, text: &str) {
        std::fs::write(self.task.path.join("base.txt"), text).unwrap();
        git(&self.task.path, &["add", "base.txt"]);
        git(&self.task.path, &["commit", "-qm", "task"]);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn target_paths_match_real_guard_and_inspection_preserves_index_and_refs() {
    let f = Fixture::new();
    f.commit_task("task\n");
    let filename = "notes with\nnewline.txt";
    std::fs::write(f.root.join(filename), "private draft").unwrap();
    git(&f.root, &["mv", "base.txt", "renamed.txt"]);
    let index = std::fs::read(f.root.join(".git/index")).unwrap();
    let head = git(&f.root, &["rev-parse", "HEAD"]);
    let status = f.task.approval_readiness().unwrap();
    let issue = status
        .issues
        .iter()
        .find(|i| i.code == "target_dirty")
        .unwrap();
    assert_eq!(issue.total_paths, 2);
    assert!(issue
        .paths
        .iter()
        .any(|p| p.path == filename && p.status == "??"));
    assert!(issue
        .paths
        .iter()
        .any(|p| p.path == "renamed.txt" && p.status == "R "));
    assert_eq!(index, std::fs::read(f.root.join(".git/index")).unwrap());
    assert_eq!(head, git(&f.root, &["rev-parse", "HEAD"]));
    let commit = git(&f.task.path, &["rev-parse", "HEAD"]);
    let error = f
        .task
        .merge_for_approval(commit.trim())
        .unwrap_err()
        .to_string();
    assert!(error.contains("2건"));
    assert!(error.contains("renamed.txt"));
}

#[test]
fn pending_edits_are_not_reported_as_a_checked_merge() {
    let f = Fixture::new();
    std::fs::write(f.task.path.join("base.txt"), "task\n").unwrap();
    std::fs::write(f.root.join("base.txt"), "dev\n").unwrap();
    git(&f.root, &["commit", "-am", "dev"]);
    let pending = f.task.approval_readiness().unwrap();
    assert_eq!(pending.source_changes, 1);
    assert!(!pending.already_integrated);
    assert!(!pending.issues.iter().any(|i| i.code == "merge_conflict"));
    git(&f.task.path, &["commit", "-am", "task"]);
    let committed = f.task.approval_readiness().unwrap();
    assert_eq!(committed.source_changes, 0);
    assert_eq!(
        committed
            .issues
            .iter()
            .find(|i| i.code == "merge_conflict")
            .unwrap()
            .paths[0]
            .path,
        "base.txt"
    );
}

#[test]
fn integrated_retry_ignores_dirty_base_and_cached_remote_counts_are_explicit() {
    let f = Fixture::new();
    git(&f.root, &["update-ref", "refs/remotes/origin/dev", "HEAD"]);
    std::fs::write(f.root.join("other.txt"), "draft").unwrap();
    let status = f.task.approval_readiness().unwrap();
    assert!(status.already_integrated);
    assert!(status.issues.is_empty());
    assert_eq!(status.remote_ahead, Some(0));
    assert_eq!(status.remote_behind, Some(0));
}

#[test]
fn foreign_target_holder_is_reported_without_inspecting_its_files() {
    let f = Fixture::new();
    f.commit_task("task\n");
    git(&f.root, &["checkout", "-b", "other"]);
    let foreign = f.root.join("foreign");
    git(
        &f.root,
        &["worktree", "add", foreign.to_str().unwrap(), "dev"],
    );
    std::fs::write(foreign.join("do-not-inspect.txt"), "draft").unwrap();
    let status = f.task.approval_readiness().unwrap();
    let issue = status
        .issues
        .iter()
        .find(|i| i.code == "target_busy")
        .unwrap();
    assert_eq!(
        Path::new(issue.location.as_ref().unwrap())
            .canonicalize()
            .unwrap(),
        foreign.canonicalize().unwrap()
    );
    assert!(issue.paths.is_empty());
}

#[cfg(unix)]
#[test]
fn failed_commit_observer_preserves_original_error_even_with_conflicting_base() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.commit_task("task\n");
    std::fs::write(f.root.join("base.txt"), "dev\n").unwrap();
    git(&f.root, &["commit", "-am", "dev"]);
    std::fs::write(f.task.path.join("doc.md"), "pending document").unwrap();
    let hooks = f.root.join("test-hooks");
    std::fs::create_dir(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho 'duplicate document id' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(
        &f.root,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    let mut stage = String::new();
    let error = f
        .task
        .approve_observed(false, |value, _| stage = value.into())
        .unwrap_err();
    assert_eq!(stage, "commit");
    assert!(error.to_string().contains("duplicate document id"));
    assert_eq!(
        f.task.conflicting_paths_against_base().unwrap(),
        vec!["base.txt"]
    );
    assert!(f.task.path.exists());
}

#[tokio::test]
async fn attempts_keep_failed_stage_after_success_and_started_without_outcome() {
    let f = Fixture::new();
    let pool = db::init_pool(f.root.join("test.sqlite").to_str().unwrap())
        .await
        .unwrap();
    let id = db::insert_task(
        &pool,
        f.root.to_str().unwrap(),
        &f.task.branch,
        "dev",
        f.task.path.to_str().unwrap(),
        "test",
        None,
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();
    let task = db::get_task(&pool, id).await.unwrap().unwrap();
    let mut first = approval::Attempt::start(&pool, &task).await.unwrap();
    first.stage = "commit".into();
    first
        .finish(&pool, &Err("document hook rejected".into()))
        .await;
    let mut second = approval::Attempt::start(&pool, &task).await.unwrap();
    second.stage = "completion".into();
    second.finish(&pool, &Ok(())).await;
    let third = approval::Attempt::start(&pool, &task).await.unwrap();
    let history = approval::history(&pool, id).await.unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].attempt_id, third.attempt_id);
    assert_eq!(history[0].outcome, "started");
    assert_eq!(history[1].outcome, "succeeded");
    assert_eq!(history[2].stage, "commit");
    assert_eq!(history[2].error.as_deref(), Some("document hook rejected"));
    assert!(!history[2].direct);
    assert!(history[2].source_sha.is_some());
    pool.close().await;
}

#[test]
fn actual_merge_conflict_retains_git_error_and_resolver_prefix() {
    let f = Fixture::new();
    f.commit_task("task\n");
    std::fs::write(f.root.join("base.txt"), "dev\n").unwrap();
    git(&f.root, &["commit", "-am", "dev"]);
    let mut stage = String::new();
    let error = f
        .task
        .approve_observed(false, |value, _| stage = value.into())
        .unwrap_err()
        .to_string();
    assert_eq!(stage, "merge");
    assert!(error.starts_with("MERGE_CONFLICT: base.txt\n"));
    assert!(error.contains("git [\"merge\""));
    assert!(f.task.path.exists());
    assert!(!f.root.join(".git/MERGE_HEAD").exists());
}
