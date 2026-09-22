use super::*;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);
const ISSUE_LIST_JSON: &str = r#"[
{"number":42,"title":"버그: 로그인 실패","labels":[{"name":"bug"}],"updatedAt":"2026-07-20T00:00:00Z"},
{"number":7,"title":"기능 요청","labels":[],"updatedAt":"2026-07-21T00:00:00Z"}
]"#;

fn write_stub(body: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let number = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = crate::testtmp::dir().join(format!("praxis-gh-stub-{}-{number}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("gh.sh");
    std::fs::write(&script, body).unwrap();
    // 실행 비트는 unix에서만 의미가 있다(윈도우 빌드에서는 os::unix가 없어 컴파일도 되지 않는다).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (dir, script)
}

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn temp_repo() -> std::path::PathBuf {
    let number = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = crate::testtmp::dir().join(format!("praxis-gh-repo-{}-{number}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    dir
}
#[test]
fn parse_issue_list_extracts_fields() {
    let issues = parse_issue_list(ISSUE_LIST_JSON).unwrap();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].number, 42);
    assert_eq!(issues[0].title, "버그: 로그인 실패");
    assert_eq!(issues[0].labels, vec![GhLabel { name: "bug".into() }]);
    assert_eq!(issues[1].labels, Vec::new());
}
#[test]
fn parse_issue_list_malformed_json_is_command_failed() {
    assert!(matches!(
        parse_issue_list("not json").unwrap_err(),
        GhError::CommandFailed(_)
    ));
}
#[test]
fn parse_issue_detail_extracts_title_and_body() {
    let detail = parse_issue_detail(r#"{"title":"제목","body":"본문 내용"}"#).unwrap();
    assert_eq!(detail.title, "제목");
    assert_eq!(detail.body, "본문 내용");
}
#[test]
fn parse_owner_repo_handles_https_and_ssh() {
    assert_eq!(
        parse_owner_repo("https://github.com/acme/widgets.git"),
        Some("acme/widgets".into())
    );
    assert_eq!(
        parse_owner_repo("git@github.com:acme/widgets.git"),
        Some("acme/widgets".into())
    );
    assert_eq!(
        parse_owner_repo("https://gitlab.com/acme/widgets.git"),
        None
    );
}
#[test]
fn looks_unauthenticated_matches_known_phrasing() {
    assert!(looks_unauthenticated(
        "To get started with GitHub CLI, please run: gh auth login"
    ));
    assert!(looks_unauthenticated(
        "You are not logged into any GitHub hosts"
    ));
    assert!(!looks_unauthenticated("issue #99 not found"));
}
#[test]
fn is_github_remote_true_for_github_origin_false_otherwise() {
    let github = temp_repo();
    git(
        &github,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
    );
    assert!(is_github_remote(&github));
    assert_eq!(remote_owner_repo(&github), Some("acme/widgets".into()));
    std::fs::remove_dir_all(github).ok();

    let gitlab = temp_repo();
    git(
        &gitlab,
        &[
            "remote",
            "add",
            "origin",
            "https://gitlab.com/acme/widgets.git",
        ],
    );
    assert!(!is_github_remote(&gitlab));
    std::fs::remove_dir_all(gitlab).ok();

    let no_remote = temp_repo();
    assert!(!is_github_remote(&no_remote));
    std::fs::remove_dir_all(no_remote).ok();
}
#[test]
fn resolve_repos_keeps_github_only_in_input_order() {
    let first = temp_repo();
    git(
        &first,
        &["remote", "add", "origin", "git@github.com:acme/first.git"],
    );
    let gitlab = temp_repo();
    git(
        &gitlab,
        &["remote", "add", "origin", "https://gitlab.com/acme/x.git"],
    );
    let second = temp_repo();
    git(
        &second,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/second.git",
        ],
    );

    let paths = vec![
        first.to_string_lossy().into_owned(),
        gitlab.to_string_lossy().into_owned(),
        second.to_string_lossy().into_owned(),
        "/does/not/exist".to_string(),
    ];
    let resolved = resolve_repos(&paths);

    assert_eq!(
        resolved
            .iter()
            .map(|repo| repo.owner_repo.as_str())
            .collect::<Vec<_>>(),
        vec!["acme/first", "acme/second"]
    );
    assert_eq!(resolved[0].path, paths[0]);

    for dir in [first, gitlab, second] {
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn resolve_repos_dedupes_same_owner_repo_keeping_first_path() {
    let original = temp_repo();
    git(
        &original,
        &["remote", "add", "origin", "https://github.com/acme/dup.git"],
    );
    let worktree = temp_repo();
    git(
        &worktree,
        &["remote", "add", "origin", "git@github.com:acme/dup.git"],
    );

    let paths = vec![
        original.to_string_lossy().into_owned(),
        worktree.to_string_lossy().into_owned(),
    ];
    let resolved = resolve_repos(&paths);

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].path, paths[0]);
    assert_eq!(resolved[0].owner_repo, "acme/dup");

    for dir in [original, worktree] {
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn list_issues_missing_binary_is_gh_unavailable() {
    let repo = temp_repo();
    let missing = repo.join("no-such-gh-binary");
    let error = list_issues_with_bin(missing.to_str().unwrap(), &repo).unwrap_err();
    assert_eq!(error, GhError::GhUnavailable);
    std::fs::remove_dir_all(repo).ok();
}
#[test]
fn list_issues_unauthenticated_stub_is_gh_unavailable() {
    let (dir, script) = write_stub(
        "#!/bin/sh\necho 'You are not logged into any GitHub hosts. Run gh auth login' >&2\nexit 1\n",
    );
    let repo = temp_repo();
    let error = list_issues_with_bin(script.to_str().unwrap(), &repo).unwrap_err();
    assert_eq!(error, GhError::GhUnavailable);
    std::fs::remove_dir_all(dir).ok();
    std::fs::remove_dir_all(repo).ok();
}
// 아래 세 건은 `gh` 자리에 sh 스크립트 스텁을 끼워 실행한다 — 윈도우에서는 실행 자체가 불가.
#[cfg(unix)]
#[test]
fn list_issues_other_failure_is_command_failed() {
    let (dir, script) = write_stub("#!/bin/sh\necho 'rate limit exceeded' >&2\nexit 1\n");
    let repo = temp_repo();
    let error = list_issues_with_bin(script.to_str().unwrap(), &repo).unwrap_err();
    assert!(matches!(error, GhError::CommandFailed(message) if message.contains("rate limit")));
    std::fs::remove_dir_all(dir).ok();
    std::fs::remove_dir_all(repo).ok();
}
#[cfg(unix)]
#[test]
fn list_issues_success_stub_parses_stdout() {
    let script_body = format!("#!/bin/sh\ncat <<'EOF'\n{ISSUE_LIST_JSON}\nEOF\n");
    let (dir, script) = write_stub(&script_body);
    let repo = temp_repo();
    assert_eq!(
        list_issues_with_bin(script.to_str().unwrap(), &repo)
            .unwrap()
            .len(),
        2
    );
    std::fs::remove_dir_all(dir).ok();
    std::fs::remove_dir_all(repo).ok();
}

#[cfg(unix)]
#[test]
fn view_issue_success_stub_parses_stdout() {
    let body = "#!/bin/sh\nprintf '%s\\n' '{\"title\":\"제목\",\"body\":\"본문\"}'\n";
    let (dir, script) = write_stub(body);
    let repo = temp_repo();
    let detail = view_issue_with_bin(script.to_str().unwrap(), &repo, 5).unwrap();
    assert_eq!(detail.title, "제목");
    assert_eq!(detail.body, "본문");
    std::fs::remove_dir_all(dir).ok();
    std::fs::remove_dir_all(repo).ok();
}

#[cfg(unix)]
#[test]
fn delete_issue_passes_yes_flag() {
    // `--yes`가 빠지면 gh가 이슈 제목을 되물으며 멈춘다 — 화면에 응답할 사람이 없다.
    let (dir, script) = write_stub("#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/args\"\n");
    let repo = temp_repo();
    delete_issue_with_bin(script.to_str().unwrap(), &repo, 42).unwrap();
    let args = std::fs::read_to_string(dir.join("args")).unwrap();
    assert_eq!(args.trim(), "issue delete 42 --yes");
    std::fs::remove_dir_all(dir).ok();
    std::fs::remove_dir_all(repo).ok();
}

#[cfg(unix)]
#[test]
fn delete_issue_permission_failure_is_command_failed() {
    // 삭제는 admin 권한을 요구한다. 사유가 사라지면 화면은 "안 지워졌다"만 말하게 된다.
    let (dir, script) =
        write_stub("#!/bin/sh\necho 'must have admin rights to Repository' >&2\nexit 1\n");
    let repo = temp_repo();
    let error = delete_issue_with_bin(script.to_str().unwrap(), &repo, 42).unwrap_err();
    assert!(matches!(error, GhError::CommandFailed(message) if message.contains("admin rights")));
    std::fs::remove_dir_all(dir).ok();
    std::fs::remove_dir_all(repo).ok();
}

#[test]
fn delete_issue_missing_binary_is_gh_unavailable() {
    let repo = temp_repo();
    let missing = repo.join("no-such-gh-binary");
    let error = delete_issue_with_bin(missing.to_str().unwrap(), &repo, 1).unwrap_err();
    assert_eq!(error, GhError::GhUnavailable);
    std::fs::remove_dir_all(repo).ok();
}

#[test]
fn build_instruction_includes_title_body_and_reference() {
    let detail = GhIssueDetail {
        title: "로그인 버그".into(),
        body: "재현 절차...".into(),
    };
    let output = build_instruction("acme/widgets", 42, &detail);
    assert!(output.contains("로그인 버그"));
    assert!(output.contains("재현 절차..."));
    assert!(output.contains("acme/widgets#42"));
}

#[test]
fn build_instruction_handles_empty_body() {
    let detail = GhIssueDetail {
        title: "제목만".into(),
        body: String::new(),
    };
    let output = build_instruction("acme/widgets", 1, &detail);
    assert!(output.contains("제목만"));
    assert!(output.contains("acme/widgets#1"));
}
