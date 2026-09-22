use serde::Serialize;

use crate::diffmodel::DiffHunk;

use super::ReviewAnnotation;

#[derive(Debug, Clone, Serialize)]
pub struct RematchedAnnotation {
    #[serde(flatten)]
    pub annotation: ReviewAnnotation,
    pub matched_hunk_id: Option<String>,
    pub orphaned: bool,
}

const REMATCH_LINE_TOLERANCE: i64 = 3;

pub fn rematch(annotations: &[ReviewAnnotation], hunks: &[DiffHunk]) -> Vec<RematchedAnnotation> {
    annotations
        .iter()
        .map(|annotation| rematch_one(annotation, hunks))
        .collect()
}

fn rematch_one(annotation: &ReviewAnnotation, hunks: &[DiffHunk]) -> RematchedAnnotation {
    if let Some(hunk) = hunks.iter().find(|hunk| hunk.id == annotation.hunk_id) {
        return RematchedAnnotation {
            annotation: annotation.clone(),
            matched_hunk_id: Some(hunk.id.clone()),
            orphaned: false,
        };
    }
    let approximate = hunks
        .iter()
        .filter(|hunk| hunk.path == annotation.path)
        .filter_map(|hunk| {
            range_distance(annotation.line, &annotation.side, hunk).map(|distance| (distance, hunk))
        })
        .filter(|(distance, _)| *distance <= REMATCH_LINE_TOLERANCE)
        .min_by_key(|(distance, _)| *distance);
    match approximate {
        Some((_, hunk)) => RematchedAnnotation {
            annotation: annotation.clone(),
            matched_hunk_id: Some(hunk.id.clone()),
            orphaned: false,
        },
        None => RematchedAnnotation {
            annotation: annotation.clone(),
            matched_hunk_id: None,
            orphaned: true,
        },
    }
}

fn range_distance(line: i64, side: &str, hunk: &DiffHunk) -> Option<i64> {
    let (start, count) = if side == "old" {
        hunk.old_range
    } else {
        hunk.new_range
    };
    let start = start as i64;
    let end = start + (count as i64).max(1) - 1;
    if line < start {
        Some(start - line)
    } else if line > end {
        Some(line - end)
    } else {
        Some(0)
    }
}
