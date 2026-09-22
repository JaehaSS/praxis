use super::operations::hunks_to_revert;
use super::*;
use crate::diffmodel::{DiffLine, RiskLevel};

fn hunk(
    path: &str,
    old_range: (u32, u32),
    new_range: (u32, u32),
    lines: Vec<DiffLine>,
    protected: bool,
) -> DiffHunk {
    DiffHunk {
        id: format!("{path}-{}-{}", new_range.0, new_range.1),
        path: path.to_string(),
        old_range,
        new_range,
        lines,
        protected,
        committed: false,
        risk: RiskLevel::Low,
    }
}

#[test]
fn compose_patch_groups_by_file_and_rebuilds_headers() {
    let first = hunk(
        "a.txt",
        (1, 3),
        (1, 3),
        vec![
            DiffLine::Context("x".into()),
            DiffLine::Del("y".into()),
            DiffLine::Add("y2".into()),
        ],
        false,
    );
    let second = hunk(
        "b.txt",
        (5, 1),
        (5, 2),
        vec![DiffLine::Context("z".into()), DiffLine::Add("w".into())],
        false,
    );
    let third = hunk(
        "a.txt",
        (20, 1),
        (20, 1),
        vec![DiffLine::Context("q".into())],
        false,
    );
    assert_eq!(
        compose_patch(&[&first, &second, &third]),
        concat!(
            "diff --git a/a.txt b/a.txt\n",
            "--- a/a.txt\n",
            "+++ b/a.txt\n",
            "@@ -1,3 +1,3 @@\n",
            " x\n",
            "-y\n",
            "+y2\n",
            "@@ -20,1 +20,1 @@\n",
            " q\n",
            "diff --git a/b.txt b/b.txt\n",
            "--- a/b.txt\n",
            "+++ b/b.txt\n",
            "@@ -5,1 +5,2 @@\n",
            " z\n",
            "+w\n",
        )
    );
}

#[test]
fn compose_patch_marks_new_file_against_dev_null() {
    let created = hunk(
        "new.txt",
        (0, 0),
        (1, 2),
        vec![DiffLine::Add("l1".into()), DiffLine::Add("l2".into())],
        false,
    );
    let patch = compose_patch(&[&created]);
    assert!(patch.contains("new file mode 100644"));
    assert!(patch.contains("--- /dev/null"));
    assert!(patch.contains("+++ b/new.txt"));
}

#[test]
fn reject_protected_errs_when_selected_hunk_is_protected() {
    let safe = hunk(
        "a.txt",
        (1, 1),
        (1, 1),
        vec![DiffLine::Context("x".into())],
        false,
    );
    let guarded = hunk(
        "secrets/.env",
        (1, 1),
        (1, 1),
        vec![DiffLine::Add("KEY=1".into())],
        true,
    );
    assert_eq!(reject_protected(&[&safe]), Ok(()));
    assert_eq!(
        reject_protected(&[&safe, &guarded]).unwrap_err(),
        PartialError::ProtectedHunkRejected(vec![guarded.id.clone()])
    );
}

fn committed_hunk(path: &str) -> DiffHunk {
    let mut hunk = hunk(
        path,
        (1, 1),
        (1, 1),
        vec![DiffLine::Add("already shipped".into())],
        false,
    );
    hunk.committed = true;
    hunk
}

#[test]
fn reject_committed_errs_when_a_committed_hunk_is_selected() {
    let pending = hunk(
        "wip.txt",
        (1, 1),
        (1, 1),
        vec![DiffLine::Add("wip".into())],
        false,
    );
    let shipped = committed_hunk("shipped.txt");

    assert!(reject_committed(&[&pending]).is_ok());
    match reject_committed(&[&pending, &shipped]) {
        Err(PartialError::CommittedHunkRejected(ids)) => assert_eq!(ids, vec![shipped.id.clone()]),
        other => panic!("커밋된 hunk 선택을 거부해야 한다: {other:?}"),
    }
}

/// 고르지 않았다는 이유로 커밋된 변경이 되돌아가면 안 된다.
///
/// 선택 거부(`reject_committed`)만으로는 이 경로를 막지 못한다 — 사용자는 그냥 고르지
/// 않으면 되고, 그러면 비선택 집합에 들어가 역패치된다.
#[test]
fn a_committed_hunk_is_never_reverted() {
    let pending = hunk(
        "wip.txt",
        (1, 1),
        (1, 1),
        vec![DiffLine::Add("wip".into())],
        false,
    );
    let shipped = committed_hunk("shipped.txt");
    let all = vec![pending.clone(), shipped.clone()];

    // 아무것도 고르지 않아도 커밋된 것은 되돌림 대상이 아니다.
    let reverted = hunks_to_revert(&all, &[]);
    assert_eq!(
        reverted.iter().map(|h| h.id.clone()).collect::<Vec<_>>(),
        vec![pending.id.clone()],
        "커밋된 hunk가 역패치 대상에 들어갔다"
    );

    // 미커밋을 고르면 되돌릴 것이 남지 않는다.
    assert!(hunks_to_revert(&all, std::slice::from_ref(&pending.id)).is_empty());
}
