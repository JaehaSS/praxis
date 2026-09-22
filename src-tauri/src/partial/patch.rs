use crate::diffmodel::{DiffHunk, DiffLine};

pub fn compose_patch(hunks: &[&DiffHunk]) -> String {
    let mut output = String::new();
    for (path, group) in group_by_path(hunks) {
        write_file_patch(&mut output, &path, &group);
    }
    output
}

pub(super) fn group_by_path<'a>(hunks: &[&'a DiffHunk]) -> Vec<(String, Vec<&'a DiffHunk>)> {
    let mut order = Vec::new();
    let mut groups: Vec<(String, Vec<&DiffHunk>)> = Vec::new();
    for hunk in hunks {
        match order.iter().position(|path| path == &hunk.path) {
            Some(index) => groups[index].1.push(hunk),
            None => {
                order.push(hunk.path.clone());
                groups.push((hunk.path.clone(), vec![hunk]));
            }
        }
    }
    groups
}

fn write_file_patch(output: &mut String, path: &str, hunks: &[&DiffHunk]) {
    let is_new_file = hunks.len() == 1 && hunks[0].old_range == (0, 0);
    let is_deleted_file = hunks.len() == 1 && hunks[0].new_range == (0, 0);
    output.push_str(&format!("diff --git a/{path} b/{path}\n"));
    if is_new_file {
        output.push_str("new file mode 100644\n--- /dev/null\n");
        output.push_str(&format!("+++ b/{path}\n"));
    } else if is_deleted_file {
        output.push_str("deleted file mode 100644\n");
        output.push_str(&format!("--- a/{path}\n+++ /dev/null\n"));
    } else {
        output.push_str(&format!("--- a/{path}\n+++ b/{path}\n"));
    }
    for hunk in hunks {
        output.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_range.0, hunk.old_range.1, hunk.new_range.0, hunk.new_range.1
        ));
        for line in &hunk.lines {
            let (prefix, text) = match line {
                DiffLine::Context(text) => (' ', text),
                DiffLine::Add(text) => ('+', text),
                DiffLine::Del(text) => ('-', text),
            };
            output.push(prefix);
            output.push_str(text);
            output.push('\n');
        }
    }
}
