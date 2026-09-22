use std::path::{Path, PathBuf};

use super::{current_branch, run_git, Worktree};

pub(super) enum RetirementTarget {
    Registered(Head),
    Missing,
    DetachedResidue,
}

/// 폐기 시점 워크트리의 HEAD가 어디에 붙어 있는가.
pub(super) enum Head {
    /// 작업 브랜치 그대로.
    Task,
    /// 워크트리 안에서 다른 브랜치로 옮겨 갔다.
    Branch(String),
    /// 어느 브랜치에도 붙어 있지 않다 (rebase·bisect·SHA 체크아웃).
    Detached,
}

pub(super) fn classify(worktree: &Worktree) -> anyhow::Result<RetirementTarget> {
    let metadata = match std::fs::symlink_metadata(&worktree.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RetirementTarget::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        anyhow::bail!("worktree path is not an isolated directory");
    }
    if worktree.is_direct() {
        return Ok(RetirementTarget::Registered(Head::Task));
    }
    let checkout = canonical(&worktree.path, "worktree")?;
    let is_registered = is_registered(&worktree.repo, &worktree.path, &checkout)?;
    let git_metadata = worktree.path.join(".git");
    let git_metadata = match std::fs::symlink_metadata(&git_metadata) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return if is_registered {
                anyhow::bail!("registered worktree is missing its .git metadata");
            } else {
                Ok(RetirementTarget::DetachedResidue)
            };
        }
        Err(error) => return Err(error.into()),
    };
    if git_metadata.file_type().is_symlink() || !git_metadata.is_file() {
        anyhow::bail!("worktree .git metadata is malformed");
    }
    if !is_registered {
        anyhow::bail!("worktree is not registered to its repository");
    }
    Ok(RetirementTarget::Registered(validate_registered_identity(
        worktree, &checkout,
    )?))
}

/// 소유권을 판정하고, 그 워크트리의 HEAD가 어디에 붙어 있는지 함께 돌려준다.
///
/// 브랜치 일치는 **소유권 속성이 아니다** — 소유권은 등록·루트·`--git-common-dir`가 이미
/// 정하고(설계 0063 D1/D3), 제거 대상은 브랜치가 아니라 경로다. 에이전트가 격리 워크트리
/// 안에서 PR용 브랜치를 만들어 갈아타는 것은 정상 작업이므로, 여기서 브랜치 불일치를
/// 실패로 두면 그 작업은 영원히 폐기할 수 없게 된다. 나머지 실패(외래 레포·미등록·깨진
/// `.git`·심볼릭 링크)는 그대로 fail-closed다.
fn validate_registered_identity(worktree: &Worktree, checkout: &Path) -> anyhow::Result<Head> {
    let root = canonical_git_path(&worktree.path, "--show-toplevel")?;
    if root != checkout {
        anyhow::bail!("worktree root does not match its registered path");
    }
    let common_dir = canonical_git_path(&worktree.path, "--git-common-dir")?;
    let expected_common_dir = canonical_git_path(&worktree.repo, "--git-common-dir")?;
    if common_dir != expected_common_dir {
        anyhow::bail!("worktree does not belong to its expected repository");
    }
    let branch = current_branch(&worktree.path)?;
    // `rev-parse --abbrev-ref`는 분리 HEAD를 문자열 "HEAD"로 표기한다 — git이 그 이름의
    // 브랜치를 허용하지 않으므로 실제 브랜치명과 헷갈릴 수 없다.
    Ok(if branch == "HEAD" {
        Head::Detached
    } else if branch == worktree.branch {
        Head::Task
    } else {
        Head::Branch(branch)
    })
}

fn canonical_git_path(cwd: &Path, arg: &str) -> anyhow::Result<PathBuf> {
    let value = run_git(cwd, &["rev-parse", arg])?;
    let path = PathBuf::from(value.trim());
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    canonical(&path, "worktree Git metadata")
}

fn canonical(path: &Path, label: &str) -> anyhow::Result<PathBuf> {
    std::fs::canonicalize(path).map_err(|error| anyhow::anyhow!("{label} is unavailable: {error}"))
}

fn is_registered(repo: &Path, path: &Path, checkout: &Path) -> anyhow::Result<bool> {
    let raw = run_git(repo, &["worktree", "list", "--porcelain", "-z"])?;
    for candidate in raw
        .split('\0')
        .filter_map(|field| field.strip_prefix("worktree "))
    {
        let candidate = Path::new(candidate);
        if candidate == path {
            return Ok(canonical(candidate, "registered worktree")? == checkout);
        }
        if canonical(candidate, "registered worktree").is_ok_and(|path| path == checkout) {
            return Ok(true);
        }
    }
    Ok(false)
}
