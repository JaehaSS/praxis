//! `ensemble::compose` 통합 테스트 — 임시 git 저장소로 forward apply 성공/충돌을 검증한다.
//! 배타 그룹·선택 검증(순수 로직)은 `src/ensemble/mod.rs`의 `#[cfg(test)]`가 담당.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::diffmodel;
use praxis_lib::ensemble::{self, ComposeError, HunkRef};
use praxis_lib::worktree;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 30줄 base 파일(`file.txt`) 커밋 1개 있는 임시 레포.
fn temp_repo() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = temp_root::dir().join(format!(
        "praxis-ensemble-compose-test-{}-{}",
        std::process::id(),
        n
    ));
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t.t"]);
    git(&dir, &["config", "user.name", "t"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("file.txt"), base_lines()).unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "base"]);
    dir
}

fn base_lines() -> String {
    (1..=30).map(|n| format!("line{n}\n")).collect()
}

fn write_lines(path: &Path, edits: &[(usize, &str)]) {
    let mut lines: Vec<String> = (1..=30).map(|n| format!("line{n}")).collect();
    for (idx, text) in edits {
        lines[*idx] = text.to_string();
    }
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

/// 심판 추천 후보(winner) worktree를 베이스로 타 후보(candidate)의 독립 hunk를 forward apply한다.
#[test]
fn compose_applies_other_candidate_hunk_onto_winner_and_rollback_restores() {
    let repo = temp_repo();
    let winner_wt = worktree::create_plain(&repo, "praxis/ensemble-winner", None).expect("create winner");
    let candidate_wt =
        worktree::create_plain(&repo, "praxis/ensemble-candidate", None).expect("create candidate");

    // winner: 상단 변경만. candidate: 하단(독립 영역) 변경만 — 겹치지 않음.
    write_lines(&winner_wt.path.join("file.txt"), &[(1, "line2-WINNER")]);
    write_lines(
        &candidate_wt.path.join("file.txt"),
        &[(24, "line25-CANDIDATE")],
    );

    let winner_diff = winner_wt.diff_unified(3).expect("winner diff");
    let candidate_diff = candidate_wt.diff_unified(3).expect("candidate diff");
    let winner_hunks = diffmodel::build_hunks(&winner_diff, &[]);
    let candidate_hunks = diffmodel::build_hunks(&candidate_diff, &[]);
    assert_eq!(winner_hunks.len(), 1);
    assert_eq!(candidate_hunks.len(), 1);

    let candidates = vec![
        (1_i64, winner_hunks.clone()),
        (2_i64, candidate_hunks.clone()),
    ];
    let selections = vec![HunkRef {
        task_id: 2,
        hunk_id: candidate_hunks[0].id.clone(),
    }];

    let outcome = ensemble::compose(1, &winner_wt, &candidates, &selections).expect("compose");
    assert_eq!(outcome.applied, selections);

    let merged = std::fs::read_to_string(winner_wt.path.join("file.txt")).unwrap();
    assert!(
        merged.contains("line2-WINNER"),
        "winner 자신의 변경은 유지: {merged}"
    );
    assert!(
        merged.contains("line25-CANDIDATE"),
        "타 후보의 선택 hunk가 forward 적용되어야 함: {merged}"
    );

    // 되돌리기는 partial 체크포인트를 그대로 재사용(reset --hard).
    winner_wt
        .restore_to_checkpoint(&outcome.checkpoint)
        .expect("rollback");
    let restored = std::fs::read_to_string(winner_wt.path.join("file.txt")).unwrap();
    assert!(restored.contains("line2-WINNER"));
    assert!(
        !restored.contains("line25-CANDIDATE"),
        "롤백은 조합 적용 이전 상태로 되돌려야 함: {restored}"
    );

    std::fs::remove_dir_all(&repo).ok();
}

/// 타 후보 hunk가 winner worktree의 컨텍스트와 어긋나(같은 영역 사전 변경) forward apply가
/// 충돌하면, 실패 hunk 목록과 함께 winner worktree는 compose 호출 직전 상태로 완전히 원복된다.
#[test]
fn compose_fails_closed_and_restores_checkpoint_on_apply_conflict() {
    let repo = temp_repo();
    let winner_wt =
        worktree::create_plain(&repo, "praxis/ensemble-winner-conflict", None).expect("create winner");
    let candidate_wt =
        worktree::create_plain(&repo, "praxis/ensemble-candidate-conflict", None).expect("create candidate");

    write_lines(
        &candidate_wt.path.join("file.txt"),
        &[(24, "line25-CANDIDATE")],
    );
    let candidate_diff = candidate_wt.diff_unified(3).expect("candidate diff");
    let candidate_hunks = diffmodel::build_hunks(&candidate_diff, &[]);
    assert_eq!(candidate_hunks.len(), 1);

    // winner worktree는 candidate hunk의 컨텍스트 라인(line24/line26)을 이미 바꿔둬 3-way 매칭이
    // 어긋나게 만든다(같은 파일 내 컨텍스트 드리프트로 인한 충돌 재현).
    write_lines(
        &winner_wt.path.join("file.txt"),
        &[(23, "line24-DRIFTED"), (25, "line26-DRIFTED")],
    );
    let pre_compose_snapshot = std::fs::read_to_string(winner_wt.path.join("file.txt")).unwrap();

    let candidates = vec![(1_i64, Vec::new()), (2_i64, candidate_hunks.clone())];
    let selections = vec![HunkRef {
        task_id: 2,
        hunk_id: candidate_hunks[0].id.clone(),
    }];

    let err = ensemble::compose(1, &winner_wt, &candidates, &selections).unwrap_err();
    match err {
        ComposeError::ApplyConflict(ids) => assert_eq!(
            ids,
            vec![HunkRef {
                task_id: 2,
                hunk_id: candidate_hunks[0].id.clone(),
            }]
        ),
        other => panic!("ApplyConflict 예상, got {other:?}"),
    }

    let restored = std::fs::read_to_string(winner_wt.path.join("file.txt")).unwrap();
    assert_eq!(
        restored, pre_compose_snapshot,
        "충돌 시 winner worktree는 compose 호출 직전 상태로 완전히 원복되어야 함"
    );

    std::fs::remove_dir_all(&repo).ok();
}
