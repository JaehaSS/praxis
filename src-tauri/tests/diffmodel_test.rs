//! diffmodel 통합 테스트 — 실제 `git diff --unified=3` 출력 형태(rename/binary/delete/멀티파일)와
//! Goal Contract protected_paths·risk 상속 연계를 검증한다. 단위 테스트(id 안정성·overlaps 기본
//! 규칙)는 `src-tauri/src/diffmodel/mod.rs`의 `#[cfg(test)]`에 있다.

use praxis_lib::diffmodel::{annotate_risk, mark_protected, parse_unified, DiffLine, RiskLevel};

fn joined(lines: &[&str]) -> String {
    lines.join("\n")
}

/// (d-1) rename만 있고 내용 변경이 없으면(similarity 100%) hunk가 생기지 않는다.
#[test]
fn rename_only_diff_produces_no_hunks() {
    let diff = joined(&[
        "diff --git a/old.txt b/new.txt",
        "similarity index 100%",
        "rename from old.txt",
        "rename to new.txt",
    ]);
    assert!(parse_unified(&diff).is_empty());
}

/// (d-2) 내용도 바뀐 rename은 새 경로(`+++ b/...`) 기준으로 hunk를 만든다.
#[test]
fn rename_with_content_change_uses_new_path() {
    let diff = joined(&[
        "diff --git a/old.txt b/new.txt",
        "similarity index 64%",
        "rename from old.txt",
        "rename to new.txt",
        "index b3c5a95..e294f3e 100644",
        "--- a/old.txt",
        "+++ b/new.txt",
        "@@ -1,5 +1,5 @@",
        " line1",
        "-line2",
        "+line2CHANGED",
        " line3",
        " line4",
        " line5",
    ]);
    let hunks = parse_unified(&diff);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].path, "new.txt");
    assert!(hunks[0].lines.contains(&DiffLine::Del("line2".into())));
    assert!(hunks[0]
        .lines
        .contains(&DiffLine::Add("line2CHANGED".into())));
}

/// (d-3) 바이너리 파일은 hunk 없이 건너뛴다(크래시 없이).
#[test]
fn binary_file_diff_produces_no_hunks() {
    let diff = joined(&[
        "diff --git a/bin.dat b/bin.dat",
        "new file mode 100644",
        "index 0000000..8352675",
        "Binary files /dev/null and b/bin.dat differ",
    ]);
    assert!(parse_unified(&diff).is_empty());
}

/// (d-4) 삭제된 파일: `+++ /dev/null`이면 옛 경로(`--- a/...`)로 귀속된다.
#[test]
fn deleted_file_diff_uses_old_path() {
    let diff = joined(&[
        "diff --git a/a.txt b/a.txt",
        "deleted file mode 100644",
        "index 83db48f..0000000",
        "--- a/a.txt",
        "+++ /dev/null",
        "@@ -1,3 +0,0 @@",
        "-line1",
        "-line2",
        "-line3",
    ]);
    let hunks = parse_unified(&diff);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].path, "a.txt");
    assert_eq!(hunks[0].new_range, (0, 0));
}

/// (d-5) 완전히 빈 diff(변경 없음)는 빈 목록.
#[test]
fn empty_diff_is_empty() {
    assert!(parse_unified("").is_empty());
    assert!(parse_unified("\n\n").is_empty());
}

/// (f) 멀티 파일 diff — 파일 순서·경로가 보존된 채로 각 hunk가 분리 파싱된다.
#[test]
fn multi_file_diff_parses_each_file_independently() {
    let diff = joined(&[
        "diff --git a/a.txt b/a.txt",
        "index b3c5a95..78e6e36 100644",
        "--- a/a.txt",
        "+++ b/a.txt",
        "@@ -1,5 +1,6 @@",
        " line1",
        "-line2",
        "+line2X",
        " line3",
        " line4",
        " line5",
        "+line6",
        "diff --git a/b.txt b/b.txt",
        "new file mode 100644",
        "index 0000000..94954ab",
        "--- /dev/null",
        "+++ b/b.txt",
        "@@ -0,0 +1,2 @@",
        "+hello",
        "+world",
    ]);
    let hunks = parse_unified(&diff);
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].path, "a.txt");
    assert_eq!(hunks[1].path, "b.txt");
    assert_eq!(hunks[1].old_range, (0, 0));
    assert_eq!(hunks[1].new_range, (1, 2));
    assert_ne!(hunks[0].id, hunks[1].id);
}

fn two_file_diff() -> String {
    joined(&[
        "diff --git a/src/auth/login.rs b/src/auth/login.rs",
        "index b3c5a95..78e6e36 100644",
        "--- a/src/auth/login.rs",
        "+++ b/src/auth/login.rs",
        "@@ -1,2 +1,2 @@",
        "-old",
        "+new",
        "diff --git a/README.md b/README.md",
        "index 1111111..2222222 100644",
        "--- a/README.md",
        "+++ b/README.md",
        "@@ -1,2 +1,2 @@",
        "-old",
        "+new",
    ])
}

/// (e) protected 패턴 매칭 — `goal_contract::protected_path_violations` 재사용 경로를
/// hunk 단위로 검증(같은 패턴 파일의 모든 hunk가 protected=true).
#[test]
fn mark_protected_flags_only_matching_paths() {
    let mut hunks = parse_unified(&two_file_diff());
    mark_protected(&mut hunks, &["src/auth/**".to_string()]);

    let auth = hunks
        .iter()
        .find(|h| h.path == "src/auth/login.rs")
        .unwrap();
    let readme = hunks.iter().find(|h| h.path == "README.md").unwrap();
    assert!(auth.protected);
    assert!(!readme.protected);
}

/// (e-2) 패턴이 비어 있으면 아무것도 protected로 표시하지 않는다.
#[test]
fn mark_protected_is_noop_without_patterns() {
    let mut hunks = parse_unified(&two_file_diff());
    mark_protected(&mut hunks, &[]);
    assert!(hunks.iter().all(|h| !h.protected));
}

/// risk 상속 — 같은 파일의 모든 hunk가 `risk::assess_blast`의 파일 단위 분류를 공유한다.
#[test]
fn annotate_risk_inherits_file_level_classification() {
    let mut hunks = parse_unified(&two_file_diff());
    annotate_risk(&mut hunks);

    let auth = hunks
        .iter()
        .find(|h| h.path == "src/auth/login.rs")
        .unwrap();
    let readme = hunks.iter().find(|h| h.path == "README.md").unwrap();
    assert_eq!(auth.risk, RiskLevel::High, "auth 경로는 high risk 상속");
    assert_eq!(readme.risk, RiskLevel::Low);
}
