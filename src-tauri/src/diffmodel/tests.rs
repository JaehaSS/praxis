use super::*;

fn single_file_modify() -> String {
    [
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
    ]
    .join("\n")
}

fn hunk_at(path: &str, new_start: u32, new_count: u32) -> DiffHunk {
    DiffHunk {
        id: format!("{path}-{new_start}-{new_count}"),
        path: path.to_string(),
        old_range: (new_start, new_count),
        new_range: (new_start, new_count),
        lines: Vec::new(),
        protected: false,
        committed: false,
        risk: RiskLevel::Low,
    }
}

#[test]
fn parses_single_file_hunk_with_classified_lines() {
    let hunks = parse_unified(&single_file_modify());
    assert_eq!(hunks.len(), 1);
    let hunk = &hunks[0];
    assert_eq!(hunk.path, "a.txt");
    assert_eq!(hunk.old_range, (1, 5));
    assert_eq!(hunk.new_range, (1, 6));
    assert_eq!(
        hunk.lines,
        vec![
            DiffLine::Context("line1".into()),
            DiffLine::Del("line2".into()),
            DiffLine::Add("line2X".into()),
            DiffLine::Context("line3".into()),
            DiffLine::Context("line4".into()),
            DiffLine::Context("line5".into()),
            DiffLine::Add("line6".into()),
        ]
    );
    assert!(!hunk.protected);
    assert_eq!(hunk.risk, RiskLevel::Low);
}

#[test]
fn hunk_id_is_stable_and_content_sensitive() {
    let source = single_file_modify();
    let first = parse_unified(&source);
    let second = parse_unified(&source);
    assert_eq!(first[0].id, second[0].id);
    let changed = parse_unified(&source.replace("line2X", "line2CHANGED"));
    assert_ne!(first[0].id, changed[0].id);
}

#[test]
fn overlaps_only_within_same_path_and_intersecting_range() {
    let left = hunk_at("a.txt", 10, 5);
    assert!(overlaps(&left, &hunk_at("a.txt", 14, 3)));
    assert!(!overlaps(&left, &hunk_at("a.txt", 20, 3)));
    assert!(!overlaps(&left, &hunk_at("b.txt", 10, 5)));
}

#[test]
fn overlaps_treats_zero_count_as_width_one() {
    assert!(overlaps(&hunk_at("a.txt", 5, 0), &hunk_at("a.txt", 5, 2)));
}

#[test]
fn empty_diff_yields_no_hunks() {
    assert!(parse_unified("").is_empty());
}
