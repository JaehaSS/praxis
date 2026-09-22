//! 세션 diff의 신규 파일 회귀 테스트.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::path::Path;
use std::process::Command;

use praxis_lib::diffmodel;
use praxis_lib::worktree;

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn session_diff_connects_an_untracked_unicode_file_to_its_hunk() {
    let repo = temp_root::dir().join(format!("praxis-untracked-diff-{}", std::process::id()));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.test"]);
    git(&repo, &["config", "user.name", "Praxis Test"]);
    std::fs::write(repo.join("seed.txt"), "seed\n").unwrap();
    git(&repo, &["add", "seed.txt"]);
    git(&repo, &["commit", "-qm", "seed"]);

    let worktree = worktree::create_plain(&repo, "praxis/untracked-unicode", None).unwrap();
    let path = "문서/신규 파일.md";
    std::fs::create_dir_all(worktree.path.join("문서")).unwrap();
    std::fs::write(worktree.path.join(path), "새 내용\n").unwrap();

    let files = worktree.diff_detailed().unwrap();
    assert!(files.iter().any(|file| file.path == path));

    let unified = worktree.diff_unified(3).unwrap();
    let hunks = diffmodel::build_hunks(&unified, &[]);
    assert!(
        hunks.iter().any(|hunk| hunk.path == path),
        "신규 파일의 실제 경로로 hunk가 연결되어야 한다: {hunks:?}"
    );

    git(&worktree.path, &["add", path]);
    let staged_hunks = diffmodel::build_hunks(&worktree.diff_unified(3).unwrap(), &[]);
    assert!(
        staged_hunks.iter().any(|hunk| hunk.path == path),
        "stage 후에도 신규 파일의 실제 경로가 유지되어야 한다: {staged_hunks:?}"
    );

    std::fs::remove_dir_all(&repo).ok();
}
