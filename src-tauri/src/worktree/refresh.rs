//! base 브랜치를 원격 최신으로 맞춘다 — fast-forward만, 실패해도 작업을 막지 않는다.
//!
//! **`pull`이 아니라 `fetch`인 이유** (플랜 0052 Phase A):
//! `git pull origin HEAD`의 `HEAD`는 **원격의 기본 브랜치**로 해석되므로 사용자가 고른 base와
//! 무관하고, `pull`은 그 브랜치가 체크아웃돼 있어야 하며, ff가 안 되면 머지 커밋이나 충돌을
//! 만든다. 작업을 시작하려던 사람이 충돌 해소부터 하게 되는 것은 최신화의 목적이 아니다.
//!
//! Tauri 비의존 — `cargo test`로 직접 검증된다.

use std::path::Path;

use serde::Serialize;

use super::{run_git, run_git_network};

/// 최신화 시도의 결과.
///
/// **성공/실패 이분법이 아니다** — 사용자가 알아야 하는 것은 "왜 안 됐는가"이고,
/// 이유마다 다음 행동이 다르다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RefreshOutcome {
    /// 원격이 없다. 로컬 전용 레포 — 알릴 것도 없다.
    Skipped,
    /// 원격에 같은 이름 브랜치가 없다. 로컬에서만 만든 브랜치.
    NoUpstream,
    /// 이미 원격과 같다.
    AlreadyCurrent,
    /// 로컬 base를 원격까지 당겼다.
    FastForwarded { commits: u32 },
    /// base가 다른 worktree에 체크아웃돼 있어 로컬 ref를 못 건드렸다.
    /// 대신 `origin/<base>`에서 직접 분기했다 — 결과물은 같다.
    BranchedFromRemote,
    /// 로컬과 원격이 갈라졌다. **아무것도 하지 않았다.**
    Diverged { ahead: u32, behind: u32 },
    /// 네트워크·인증·타임아웃. 로컬 base 그대로 진행했다.
    Failed { reason: String },
}

impl RefreshOutcome {
    /// 사용자에게 보일 만한 일인가. `Skipped`·`AlreadyCurrent`는 정상이라 조용하다.
    ///
    /// 정상 동작을 매번 보고하면 그 줄은 곧 안 읽힌다.
    pub fn is_noteworthy(&self) -> bool {
        !matches!(self, Self::Skipped | Self::AlreadyCurrent)
    }

    /// 분기 기준을 `origin/<base>`로 바꿔야 하는가.
    pub fn prefers_remote_ref(&self) -> bool {
        matches!(self, Self::BranchedFromRemote)
    }

    /// 계측 표본에 남길 종류 이름. **serde 태그(`kind`)와 같은 문자열을 쓴다** — 두 이름이
    /// 갈라지면 같은 결과가 표본과 이벤트에서 다르게 세어져 집계가 조용히 어긋난다.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::NoUpstream => "no_upstream",
            Self::AlreadyCurrent => "already_current",
            Self::FastForwarded { .. } => "fast_forwarded",
            Self::BranchedFromRemote => "branched_from_remote",
            Self::Diverged { .. } => "diverged",
            Self::Failed { .. } => "failed",
        }
    }
}

/// base 브랜치가 체크아웃돼 있는 자리.
#[derive(Debug, PartialEq, Eq)]
enum Checkout {
    /// 이 레포의 메인 체크아웃이 쥐고 있다.
    ThisRepo,
    /// 다른 worktree가 쥐고 있다.
    OtherWorktree,
    /// 아무도 안 쓴다 — ref만 옮기면 된다.
    Nowhere,
}

/// base 브랜치를 원격 최신으로 맞춘다.
///
/// **절대 `Err`를 반환하지 않는다** — 모든 실패는 `Failed`로 접혀 호출부가 계속 진행한다.
/// 비행기 안이든 VPN이 끊겼든 코드는 짜야 한다(worktree 계약 4).
pub fn refresh_base(repo: &Path, base: &str) -> RefreshOutcome {
    match try_refresh(repo, base) {
        Ok(outcome) => outcome,
        Err(e) => RefreshOutcome::Failed {
            reason: e.to_string(),
        },
    }
}

fn try_refresh(repo: &Path, base: &str) -> anyhow::Result<RefreshOutcome> {
    if !has_origin(repo)? {
        return Ok(RefreshOutcome::Skipped);
    }
    // **먼저 물어보고 나중에 가져온다.** `git fetch origin <없는브랜치>`는 조용히 넘어가지 않고
    // `couldn't find remote ref`로 **실패한다** — 그대로 두면 "원격에 없는 브랜치"와 "네트워크가
    // 끊겼다"가 같은 Failed로 뭉개진다. 사용자가 할 일이 완전히 다른 두 상황이다.
    //
    // `ls-remote`는 객체를 안 옮기므로 싸고, 연결 확인까지 겸한다 — 여기서 실패하면 그것이
    // 곧 네트워크·인증 실패다.
    let listed = run_git_network(repo, &["ls-remote", "--heads", "origin", base])?;
    if listed.trim().is_empty() {
        return Ok(RefreshOutcome::NoUpstream);
    }

    // 원격 ref만 갱신한다. 이 단계는 로컬 브랜치를 절대 건드리지 않으므로
    // 아래 분기 판단을 안전하게 할 수 있다.
    run_git_network(repo, &["fetch", "--no-tags", "origin", base])?;

    let remote = format!("origin/{base}");
    // fetch가 성공했는데 원격 추적 ref가 없다면 refspec 설정이 특이한 레포다 — 조용히 틀린
    // 비교(ahead/behind가 엉뚱한 값)를 하느니 손대지 않는다.
    if !remote_branch_exists(repo, base) {
        return Ok(RefreshOutcome::NoUpstream);
    }
    let (ahead, behind) = ahead_behind(repo, base, &remote)?;
    if ahead == 0 && behind == 0 {
        return Ok(RefreshOutcome::AlreadyCurrent);
    }
    // 로컬에만 있는 커밋이 하나라도 있으면 ff가 성립하지 않는다. 강제로 맞추면
    // 사용자의 커밋이 사라지므로, 작업 시작 절차가 할 일이 아니다.
    if ahead > 0 {
        return Ok(RefreshOutcome::Diverged { ahead, behind });
    }

    match checkout_location(repo, base)? {
        // 이 레포가 base를 쥐고 있다 — 워킹트리를 건드리므로 --ff-only로 한다.
        Checkout::ThisRepo => {
            run_git(repo, &["merge", "--ff-only", &remote])?;
            Ok(RefreshOutcome::FastForwarded { commits: behind })
        }
        // 아무도 안 쓰는 브랜치 — ref만 직접 옮긴다. 워킹트리가 없어 가장 싸고 안전하다.
        // `<src>:<dst>` refspec은 기본이 ff-only라 갈라진 경우 git이 거부한다(여기선 이미 걸렀다).
        Checkout::Nowhere => {
            run_git_network(repo, &["fetch", "origin", &format!("{base}:{base}")])?;
            Ok(RefreshOutcome::FastForwarded { commits: behind })
        }
        // 다른 worktree가 쥐고 있다 — 그 워킹트리를 여기서 바꾸면 거기서 일하던 사람(또는
        // 에이전트)의 발밑이 무너진다. 로컬 ref는 두고 원격에서 분기한다.
        Checkout::OtherWorktree => Ok(RefreshOutcome::BranchedFromRemote),
    }
}

fn has_origin(repo: &Path) -> anyhow::Result<bool> {
    Ok(run_git(repo, &["remote"])?
        .lines()
        .any(|l| l.trim() == "origin"))
}

fn remote_branch_exists(repo: &Path, branch: &str) -> bool {
    std::process::Command::new("git")
        .current_dir(repo)
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{branch}"),
        ])
        .status()
        .is_ok_and(|s| s.success())
}

/// `(로컬에만 있는 수, 원격에만 있는 수)`.
fn ahead_behind(repo: &Path, local: &str, remote: &str) -> anyhow::Result<(u32, u32)> {
    let out = run_git(
        repo,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{local}...{remote}"),
        ],
    )?;
    let mut parts = out.split_whitespace();
    let ahead = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let behind = parts.next().unwrap_or("0").parse().unwrap_or(0);
    Ok((ahead, behind))
}

/// `git worktree list --porcelain`을 읽어 이 브랜치를 누가 쥐고 있는지 본다.
///
/// 첫 레코드가 메인 체크아웃이다. `branch refs/heads/<name>` 줄이 그 worktree가 쥔 브랜치이고,
/// detached HEAD면 그 줄이 없다.
fn checkout_location(repo: &Path, branch: &str) -> anyhow::Result<Checkout> {
    let raw = run_git(repo, &["worktree", "list", "--porcelain"])?;
    let want = format!("branch refs/heads/{branch}");
    let mut first = true;
    let mut found: Option<bool> = None;
    for record in raw.split("\n\n") {
        if record.trim().is_empty() {
            continue;
        }
        if record.lines().any(|l| l.trim() == want) {
            found = Some(first);
            break;
        }
        first = false;
    }
    Ok(match found {
        Some(true) => Checkout::ThisRepo,
        Some(false) => Checkout::OtherWorktree,
        None => Checkout::Nowhere,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} 실패: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn tmp(tag: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let d = crate::testtmp::dir().join(format!(
            "praxis-refresh-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn commit(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), name).unwrap();
        git(dir, &["add", "-A"]);
        git(
            dir,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@t",
                "commit",
                "-m",
                name,
            ],
        );
    }

    /// `origin`이 로컬 경로인 (upstream, clone) 쌍. 네트워크 없이 fetch 의미론을 전부 재현한다.
    fn pair(tag: &str) -> (PathBuf, PathBuf) {
        let up = tmp(&format!("{tag}-up"));
        git(&up, &["init", "-b", "main"]);
        commit(&up, "a.txt");

        let clone = tmp(&format!("{tag}-clone"));
        std::fs::remove_dir_all(&clone).unwrap();
        git(
            crate::testtmp::dir().as_path(),
            &[
                "clone",
                "-q",
                up.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        (up, clone)
    }

    #[test]
    fn no_remote_is_skipped_not_failed() {
        // 로컬 전용 레포에서 매번 경고가 뜨면 경고가 무의미해진다.
        let d = tmp("noremote");
        git(&d, &["init", "-b", "main"]);
        commit(&d, "a.txt");
        assert_eq!(refresh_base(&d, "main"), RefreshOutcome::Skipped);
    }

    #[test]
    fn already_current_when_nothing_moved() {
        let (_up, clone) = pair("current");
        assert_eq!(refresh_base(&clone, "main"), RefreshOutcome::AlreadyCurrent);
    }

    #[test]
    fn fast_forwards_the_currently_checked_out_base() {
        let (up, clone) = pair("ffhere");
        commit(&up, "b.txt");
        assert_eq!(
            refresh_base(&clone, "main"),
            RefreshOutcome::FastForwarded { commits: 1 }
        );
        assert!(clone.join("b.txt").exists(), "워킹트리가 안 따라왔다");
    }

    #[test]
    fn fast_forwards_a_branch_that_is_not_checked_out() {
        let (up, clone) = pair("ffref");
        git(&up, &["checkout", "-q", "-b", "feat"]);
        commit(&up, "f.txt");
        // clone은 main을 체크아웃한 채로 feat만 만들어 둔다.
        git(&clone, &["fetch", "-q", "origin", "feat:feat"]);
        commit(&up, "f2.txt");

        let before = run_git(&clone, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(
            refresh_base(&clone, "feat"),
            RefreshOutcome::FastForwarded { commits: 1 }
        );
        let after_feat = run_git(&clone, &["rev-parse", "feat"]).unwrap();
        let up_feat = run_git(&up, &["rev-parse", "feat"]).unwrap();
        assert_eq!(after_feat, up_feat, "로컬 ref가 원격까지 안 갔다");
        assert_eq!(
            before,
            run_git(&clone, &["rev-parse", "HEAD"]).unwrap(),
            "체크아웃 안 된 브랜치를 옮기면서 HEAD가 움직였다"
        );
    }

    #[test]
    fn diverged_history_is_left_untouched() {
        // 사용자 커밋 유실을 막는 유일한 방어선이다.
        let (up, clone) = pair("diverged");
        commit(&up, "remote.txt");
        commit(&clone, "local.txt");
        let before = run_git(&clone, &["rev-parse", "HEAD"]).unwrap();

        assert_eq!(
            refresh_base(&clone, "main"),
            RefreshOutcome::Diverged {
                ahead: 1,
                behind: 1
            }
        );
        assert_eq!(
            before,
            run_git(&clone, &["rev-parse", "HEAD"]).unwrap(),
            "갈라진 이력을 건드렸다"
        );
        assert!(clone.join("local.txt").exists(), "로컬 커밋이 사라졌다");
    }

    #[test]
    fn a_branch_with_no_remote_counterpart_reports_no_upstream() {
        let (_up, clone) = pair("noupstream");
        git(&clone, &["checkout", "-q", "-b", "local-only"]);
        assert_eq!(refresh_base(&clone, "local-only"), RefreshOutcome::NoUpstream);
    }

    #[test]
    fn a_base_held_by_another_worktree_branches_from_the_remote() {
        let (up, clone) = pair("otherwt");
        git(&up, &["checkout", "-q", "-b", "shared"]);
        commit(&up, "s.txt");
        git(&clone, &["fetch", "-q", "origin", "shared:shared"]);
        commit(&up, "s2.txt");

        // 다른 worktree가 shared를 쥐게 한다.
        let wt = tmp("otherwt-wt");
        std::fs::remove_dir_all(&wt).unwrap();
        git(
            &clone,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "shared"],
        );

        let before = run_git(&clone, &["rev-parse", "shared"]).unwrap();
        assert_eq!(
            refresh_base(&clone, "shared"),
            RefreshOutcome::BranchedFromRemote
        );
        assert_eq!(
            before,
            run_git(&clone, &["rev-parse", "shared"]).unwrap(),
            "다른 worktree가 쥔 브랜치를 옮겼다"
        );
    }

    #[test]
    fn unreachable_remote_fails_without_hanging() {
        let (_up, clone) = pair("unreachable");
        git(
            &clone,
            &[
                "remote",
                "set-url",
                "origin",
                "/nonexistent/praxis-not-a-repo",
            ],
        );
        let started = std::time::Instant::now();
        let outcome = refresh_base(&clone, "main");
        assert!(
            matches!(outcome, RefreshOutcome::Failed { .. }),
            "닿을 수 없는 원격인데 {outcome:?}"
        );
        assert!(
            started.elapsed() < super::super::NETWORK_GIT_TIMEOUT,
            "타임아웃까지 매달렸다"
        );
    }

    #[test]
    fn only_skipped_and_current_are_quiet() {
        assert!(!RefreshOutcome::Skipped.is_noteworthy());
        assert!(!RefreshOutcome::AlreadyCurrent.is_noteworthy());
        assert!(RefreshOutcome::NoUpstream.is_noteworthy());
        assert!(RefreshOutcome::FastForwarded { commits: 1 }.is_noteworthy());
        assert!(RefreshOutcome::BranchedFromRemote.is_noteworthy());
        assert!(RefreshOutcome::Diverged { ahead: 1, behind: 1 }.is_noteworthy());
        assert!(RefreshOutcome::Failed { reason: "x".into() }.is_noteworthy());
    }

    /// 표본 이름과 serde 태그가 갈라지면 집계가 조용히 어긋난다 — 한 변종씩 직렬화해
    /// 두 이름이 같은지 확인한다.
    #[test]
    fn the_sample_label_matches_the_serde_tag() {
        for o in [
            RefreshOutcome::Skipped,
            RefreshOutcome::NoUpstream,
            RefreshOutcome::AlreadyCurrent,
            RefreshOutcome::FastForwarded { commits: 3 },
            RefreshOutcome::BranchedFromRemote,
            RefreshOutcome::Diverged { ahead: 1, behind: 2 },
            RefreshOutcome::Failed {
                reason: "네트워크 없음".into(),
            },
        ] {
            let tagged = serde_json::to_value(&o).expect("RefreshOutcome은 직렬화된다");
            assert_eq!(
                tagged["kind"], o.label(),
                "{o:?}의 표본 이름이 serde 태그와 다르다"
            );
        }
    }

    #[test]
    fn only_branched_from_remote_changes_the_start_point() {
        assert!(RefreshOutcome::BranchedFromRemote.prefers_remote_ref());
        for o in [
            RefreshOutcome::Skipped,
            RefreshOutcome::NoUpstream,
            RefreshOutcome::AlreadyCurrent,
            RefreshOutcome::FastForwarded { commits: 3 },
            RefreshOutcome::Diverged { ahead: 1, behind: 1 },
        ] {
            assert!(!o.prefers_remote_ref(), "{o:?}가 원격 ref를 요구했다");
        }
    }
}
