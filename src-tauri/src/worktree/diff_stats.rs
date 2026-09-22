use super::FileDiff;

pub(super) fn format_diff_stat(files: &[FileDiff]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut additions = 0;
    let mut deletions = 0;
    let mut lines = Vec::with_capacity(files.len() + 1);
    for file in files {
        let counts = patch_counts(&file.patch);
        additions += counts.0;
        deletions += counts.1;
        lines.push(stat_line(file, counts));
    }
    lines.push(stat_summary(files.len(), additions, deletions));
    lines.join("\n") + "\n"
}

fn patch_counts(patch: &str) -> (usize, usize) {
    patch.lines().fold((0, 0), |(additions, deletions), line| {
        if line.starts_with('+') && !line.starts_with("+++") {
            (additions + 1, deletions)
        } else if line.starts_with('-') && !line.starts_with("---") {
            (additions, deletions + 1)
        } else {
            (additions, deletions)
        }
    })
}

fn stat_line(file: &FileDiff, counts: (usize, usize)) -> String {
    if file.patch.contains("Binary files ") || file.patch.contains("GIT binary patch") {
        return format!(" {} | Bin", file.path);
    }
    let total = counts.0 + counts.1;
    let pluses = "+".repeat(counts.0.min(20));
    let minuses = "-".repeat(counts.1.min(20));
    format!(" {} | {} {}{}", file.path, total, pluses, minuses)
}

fn stat_summary(files: usize, additions: usize, deletions: usize) -> String {
    let file_word = if files == 1 { "file" } else { "files" };
    let mut parts = vec![format!("{files} {file_word} changed")];
    if additions > 0 {
        parts.push(format!("{additions} insertions(+)"));
    }
    if deletions > 0 {
        parts.push(format!("{deletions} deletions(-)"));
    }
    parts.join(", ")
}
