use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use super::{GhError, GhIssue, GhIssueDetail, GhRepo};

/// 경로당 `git remote get-url`을 한 번씩 띄우므로 후보 수에 상한을 둔다. 홈의 레포 버튼은
/// 최근 몇 개만 쓰이므로 이 이상은 화면에도 담기지 않는다.
const REPO_RESOLVE_LIMIT: usize = 20;

fn gh_on_path() -> Option<String> {
    crate::reviewer::which("gh")
}

pub(super) fn looks_unauthenticated(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    [
        "gh auth login",
        "not logged into",
        "authentication required",
        "no oauth token",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn remote_url(repo_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!url.is_empty()).then_some(url)
}

pub fn parse_owner_repo(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches(".git");
    if let Some(repository) = url.strip_prefix("git@github.com:") {
        return (!repository.is_empty()).then(|| repository.to_string());
    }
    let index = url.find("github.com/")?;
    let repository = &url[index + "github.com/".len()..];
    (!repository.is_empty()).then(|| repository.to_string())
}

pub fn is_github_remote(repo_path: &Path) -> bool {
    remote_url(repo_path).is_some_and(|url| parse_owner_repo(&url).is_some())
}

pub fn remote_owner_repo(repo_path: &Path) -> Option<String> {
    remote_url(repo_path).and_then(|url| parse_owner_repo(&url))
}

/// 후보 경로들 중 GitHub 레포인 것만, 입력 순서를 지켜 추린다. 네트워크를 타지 않는다
/// (`git remote get-url`만 실행) — 홈이 뜰 때마다 도는 경로라 `gh`를 부르면 안 된다.
///
/// 같은 `owner/repo`를 가리키는 경로가 여러 개일 수 있다(워크트리·클론 중복). 이슈 목록은
/// 어느 쪽에서 조회해도 같으므로 먼저 온 경로만 남긴다 — 버튼이 같은 이름으로 두 번 뜨는 것을 막는다.
pub fn resolve_repos(paths: &[String]) -> Vec<GhRepo> {
    let mut seen = HashSet::new();
    paths
        .iter()
        .take(REPO_RESOLVE_LIMIT)
        .filter_map(|path| {
            let owner_repo = remote_owner_repo(Path::new(path))?;
            seen.insert(owner_repo.clone()).then(|| GhRepo {
                path: path.clone(),
                owner_repo,
            })
        })
        .collect()
}

pub(super) fn parse_issue_list(json: &str) -> Result<Vec<GhIssue>, GhError> {
    serde_json::from_str(json)
        .map_err(|error| GhError::CommandFailed(format!("이슈 목록 파싱 실패: {error}")))
}

pub(super) fn parse_issue_detail(json: &str) -> Result<GhIssueDetail, GhError> {
    serde_json::from_str(json)
        .map_err(|error| GhError::CommandFailed(format!("이슈 상세 파싱 실패: {error}")))
}

fn run_gh(gh_bin: &str, repo_path: &Path, args: &[&str]) -> Result<String, GhError> {
    let output = Command::new(gh_bin)
        .args(args)
        .current_dir(repo_path)
        .output()
        .map_err(|_| GhError::GhUnavailable)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if looks_unauthenticated(&stderr) {
            return Err(GhError::GhUnavailable);
        }
        return Err(GhError::CommandFailed(stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn list_issues_with_bin(
    gh_bin: &str,
    repo_path: &Path,
) -> Result<Vec<GhIssue>, GhError> {
    let stdout = run_gh(
        gh_bin,
        repo_path,
        &[
            "issue",
            "list",
            "--json",
            "number,title,labels,updatedAt",
            "--limit",
            "30",
        ],
    )?;
    parse_issue_list(&stdout)
}

pub(super) fn view_issue_with_bin(
    gh_bin: &str,
    repo_path: &Path,
    number: u64,
) -> Result<GhIssueDetail, GhError> {
    let number = number.to_string();
    let stdout = run_gh(
        gh_bin,
        repo_path,
        &["issue", "view", &number, "--json", "title,body"],
    )?;
    parse_issue_detail(&stdout)
}

pub fn list_issues(repo_path: &Path) -> Result<Vec<GhIssue>, GhError> {
    let binary = gh_on_path().ok_or(GhError::GhUnavailable)?;
    list_issues_with_bin(&binary, repo_path)
}

pub(super) fn parse_pr_url(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let url = value.get("url")?.as_str()?.trim();
    (!url.is_empty()).then(|| url.to_string())
}

/// 브랜치에 열린 PR의 URL. **없는 것이 정상**(AwaitingReview 시점엔 대개 PR이 아직 없다)이므로
/// gh 부재·미인증·PR 미존재를 모두 None으로 접는다 — 호출측은 링크를 생략하면 그만이다.
/// blocking(프로세스 spawn)이라 async 문맥에서는 `spawn_blocking`으로 감싸 부른다.
pub fn pr_url_for_branch(repo_path: &Path, branch: &str) -> Option<String> {
    let binary = gh_on_path()?;
    let stdout = run_gh(&binary, repo_path, &["pr", "view", branch, "--json", "url"]).ok()?;
    parse_pr_url(&stdout)
}

pub fn view_issue(repo_path: &Path, number: u64) -> Result<GhIssueDetail, GhError> {
    let binary = gh_on_path().ok_or(GhError::GhUnavailable)?;
    view_issue_with_bin(&binary, repo_path, number)
}

pub(super) fn delete_issue_with_bin(
    gh_bin: &str,
    repo_path: &Path,
    number: u64,
) -> Result<(), GhError> {
    let number = number.to_string();
    // `--yes`가 없으면 gh가 이슈 제목을 되물으며 stdin에서 멈춘다 — 확인은 화면이 이미 받았다.
    run_gh(gh_bin, repo_path, &["issue", "delete", &number, "--yes"])?;
    Ok(())
}

/// 이슈를 GitHub에서 **완전히 삭제**한다(close가 아니다). 레포 admin 권한이 필요하므로
/// 권한 부족은 `CommandFailed`로 올라온다 — 호출측이 그 사유를 그대로 보여준다.
pub fn delete_issue(repo_path: &Path, number: u64) -> Result<(), GhError> {
    let binary = gh_on_path().ok_or(GhError::GhUnavailable)?;
    delete_issue_with_bin(&binary, repo_path, number)
}
