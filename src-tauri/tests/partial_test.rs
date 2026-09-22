//! `partial` 모듈 통합 테스트 — 임시 git 저장소로 apply/rollback/충돌을 검증한다.
//! 순수 로직(patch 합성·protected 거부)은 `src/partial/mod.rs`의 `#[cfg(test)]`가 담당.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::diffmodel;
use praxis_lib::partial::{self, PartialError};
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

/// 30줄 base 파일(`file.txt`) 커밋 1개 있는 임시 레포 — 상단/하단에 독립 hunk 2개를 만들 여유.
fn temp_repo() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir =
        temp_root::dir().join(format!("praxis-partial-test-{}-{}", std::process::id(), n));
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

/// 기본 흐름: 선택 hunk만 worktree에 남고, 비선택 hunk는 역패치로 제거된다.
/// 이후 롤백하면 체크포인트(둘 다 있던 상태)로 완전히 복원된다.
#[test]
fn apply_keeps_only_selected_hunks_and_rollback_restores_all() {
    let repo = temp_repo();
    let wt = worktree::create_plain(&repo, "praxis/partial-basic", None).expect("create");
    let file = wt.path.join("file.txt");
    // 상단(index 1=line2)·하단(index 24=line25) 독립 변경 — 기본 context=3 간격을 두어 별개 hunk.
    write_lines(&file, &[(1, "line2-CHANGED"), (24, "line25-CHANGED")]);

    let diff = wt.diff_unified(3).expect("diff_unified");
    let hunks = diffmodel::build_hunks(&diff, &[]);
    assert_eq!(
        hunks.len(),
        2,
        "독립 변경 2건은 별개 hunk여야 함: {hunks:?}"
    );

    let keep = hunks.iter().find(|h| h.new_range.0 < 15).expect("top hunk");
    let discard = hunks
        .iter()
        .find(|h| h.new_range.0 >= 15)
        .expect("bottom hunk");

    let outcome = partial::apply(&wt, &hunks, std::slice::from_ref(&keep.id)).expect("apply");
    assert_eq!(outcome.kept_hunk_ids, vec![keep.id.clone()]);
    assert_eq!(outcome.discarded_hunk_ids, vec![discard.id.clone()]);

    let content = std::fs::read_to_string(&file).unwrap();
    assert!(content.contains("line2-CHANGED"), "content: {content}");
    assert!(
        !content.contains("line25-CHANGED"),
        "비선택 hunk는 역패치로 제거되어야 함: {content}"
    );

    partial::rollback(&wt, &outcome.checkpoint).expect("rollback");
    let restored = std::fs::read_to_string(&file).unwrap();
    assert!(restored.contains("line2-CHANGED"));
    assert!(
        restored.contains("line25-CHANGED"),
        "롤백은 폐기된 hunk까지 복원해야 함: {restored}"
    );

    std::fs::remove_dir_all(&repo).ok();
}

/// (c) protected hunk가 선택(유지) 대상에 포함되면 체크포인트 생성 전에 거부되고 상태는 불변이다.
#[test]
fn apply_rejects_protected_hunk_in_selection_without_side_effects() {
    let repo = temp_repo();
    let wt = worktree::create_plain(&repo, "praxis/partial-protected", None).expect("create");
    let file = wt.path.join("file.txt");
    write_lines(&file, &[(1, "line2-CHANGED")]);

    let diff_before = wt.diff_unified(3).expect("diff_unified");
    let mut hunks = diffmodel::build_hunks(&diff_before, &[]);
    assert_eq!(hunks.len(), 1);
    hunks[0].protected = true;

    let err = partial::apply(&wt, &hunks, &[hunks[0].id.clone()]).unwrap_err();
    assert_eq!(
        err,
        PartialError::ProtectedHunkRejected(vec![hunks[0].id.clone()])
    );

    // 상태 불변: 체크포인트 커밋이 생기지 않았고(HEAD 그대로) 워킹트리 diff도 그대로.
    let head_after = Command::new("git")
        .current_dir(&wt.path)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let base_head = Command::new("git")
        .current_dir(&repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&head_after.stdout),
        String::from_utf8_lossy(&base_head.stdout),
        "protected 거부 시 체크포인트 커밋이 생기면 안 됨"
    );
    let diff_after = wt.diff_unified(3).expect("diff_unified");
    assert_eq!(
        diff_before, diff_after,
        "protected 거부 후 워킹트리 변경 없음"
    );

    std::fs::remove_dir_all(&repo).ok();
}

/// (b) 같은 파일 내 의존 hunk 충돌 — 비선택 hunk의 컨텍스트가 hunk 계산 이후 어긋나면
/// 역패치가 실패하고, 실패 hunk 목록과 함께 워킹트리는 체크포인트(=apply 호출 직전 상태)로
/// 완전히 원복된다(상태 불변).
#[test]
fn apply_fails_closed_and_restores_checkpoint_on_reverse_patch_conflict() {
    let repo = temp_repo();
    let wt = worktree::create_plain(&repo, "praxis/partial-conflict", None).expect("create");
    let file = wt.path.join("file.txt");
    write_lines(&file, &[(1, "line2-CHANGED"), (24, "line25-CHANGED")]);

    let diff = wt.diff_unified(3).expect("diff_unified");
    let hunks = diffmodel::build_hunks(&diff, &[]);
    assert_eq!(hunks.len(), 2);
    let keep = hunks
        .iter()
        .find(|h| h.new_range.0 < 15)
        .expect("top hunk")
        .clone();
    let discard = hunks
        .iter()
        .find(|h| h.new_range.0 >= 15)
        .expect("bottom hunk")
        .clone();

    // hunk 계산 이후 discard의 컨텍스트 영역(line27)을 추가로 바꿔 역패치 매칭을 깨뜨린다
    // — "같은 파일 내 의존 hunk" 충돌 재현. 이 시점 상태를 apply() 직전 스냅샷으로 보존.
    write_lines(
        &file,
        &[
            (1, "line2-CHANGED"),
            (24, "line25-CHANGED"),
            (26, "line27-EXTRA"),
        ],
    );
    let pre_apply_snapshot = std::fs::read_to_string(&file).unwrap();

    let err = partial::apply(&wt, &hunks, std::slice::from_ref(&keep.id)).unwrap_err();
    match err {
        PartialError::ApplyConflict(ids) => assert_eq!(ids, vec![discard.id.clone()]),
        other => panic!("ApplyConflict 예상, got {other:?}"),
    }

    let restored = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        restored, pre_apply_snapshot,
        "충돌 시 워킹트리는 apply 호출 직전 상태(체크포인트)로 완전히 원복되어야 함"
    );

    std::fs::remove_dir_all(&repo).ok();
}
