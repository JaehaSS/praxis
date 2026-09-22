use crate::diffmodel::{DiffHunk, DiffLine};

use super::ReviewAnnotation;

pub struct ResendItem {
    pub path: String,
    pub line: i64,
    pub quoted: Option<String>,
    pub comment: String,
}

pub fn build_resend_items(annotations: &[ReviewAnnotation], hunks: &[DiffHunk]) -> Vec<ResendItem> {
    annotations
        .iter()
        .map(|annotation| ResendItem {
            path: annotation.path.clone(),
            line: annotation.line,
            quoted: hunks
                .iter()
                .find(|hunk| hunk.id == annotation.hunk_id)
                .and_then(|hunk| quote_line(hunk, annotation.line, &annotation.side)),
            comment: annotation.body_md.clone(),
        })
        .collect()
}

pub fn format_resend(items: &[ResendItem]) -> String {
    let mut output = format!(
        "[리뷰 주석 {}건 — 각 항목을 반영하고 완료 후 보고할 것]\n",
        items.len()
    );
    for (index, item) in items.iter().enumerate() {
        output.push_str(&format!(
            "{}. {}:{} (변경 후 기준)\n",
            index + 1,
            item.path,
            item.line
        ));
        if let Some(quoted) = &item.quoted {
            output.push_str(&format!("   > {quoted}\n"));
        }
        output.push_str(&format!("   코멘트: {}\n", item.comment));
    }
    output
}

pub fn quote_line(hunk: &DiffHunk, line: i64, side: &str) -> Option<String> {
    let mut old_line = hunk.old_range.0 as i64;
    let mut new_line = hunk.new_range.0 as i64;
    for diff_line in &hunk.lines {
        match diff_line {
            DiffLine::Context(text) => {
                if (side == "old" && old_line == line) || (side != "old" && new_line == line) {
                    return Some(format!("  {text}"));
                }
                old_line += 1;
                new_line += 1;
            }
            DiffLine::Del(text) => {
                if side == "old" && old_line == line {
                    return Some(format!("- {text}"));
                }
                old_line += 1;
            }
            DiffLine::Add(text) => {
                if side != "old" && new_line == line {
                    return Some(format!("+ {text}"));
                }
                new_line += 1;
            }
        }
    }
    None
}
