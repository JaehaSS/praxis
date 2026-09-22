//! unified diff 압축 — 리뷰 프롬프트 예산 안에서 **모든 변경 파일이 살아남도록** 재배분한다.
//!
//! 기존 `truncate(diff, N)`은 앞에서부터 N바이트를 자르고 뒤를 버렸다. diff는 경로 순으로
//! 붙기 때문에 `Cargo.lock` 하나가 예산을 다 먹으면 그 뒤 `src/**`가 통째로 사라지고, 리뷰어는
//! 보지도 못한 변경을 통과시킨다. 여기서는 두 단계로 그 실패를 없앤다.
//!
//! 1. **락파일 요약** — 잠금 파일 hunk는 리뷰 가치가 거의 없으므로 `+A/-B` 한 줄로 접는다.
//! 2. **파일별 균등 배분** — 그래도 예산을 넘으면 파일 수로 나눠 각 파일에 몫을 주고, 몫을
//!    넘는 파일만 hunk 경계에서 자른다. 어떤 파일도 목록에서 사라지지 않는다.
//!
//! 원본 회수용 캐시는 두지 않는다. Praxis의 diff는 worktree에서 `git diff`로 언제든
//! 결정론적으로 재생성되므로, 무엇이 얼마나 접혔는지만 알려 주면 충분하다.
//!
//! 압축 결과가 원본보다 커지면 **원본을 그대로 돌려준다** — 압축이 손해를 내는 일은 없다.

/// 잠금 파일 이름. 경로 마지막 구간이 이 중 하나면 hunk 본문을 요약한다.
const LOCKFILES: [&str; 9] = [
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "poetry.lock",
    "Gemfile.lock",
    "composer.lock",
    "go.sum",
];

/// 파일 하나에 배분할 최소 예산(bytes). 파일이 아주 많아도 헤더+몇 줄은 남긴다.
const MIN_FILE_BUDGET: usize = 512;

/// 압축 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedDiff {
    /// 프롬프트에 실을 본문.
    pub text: String,
    /// 압축 전 바이트.
    pub original_bytes: usize,
    /// 무엇을 접었는지 사람이 읽을 수 있는 기록. 비어 있으면 무손실.
    pub notes: Vec<String>,
}

impl CompressedDiff {
    /// 손실 없이 원본을 그대로 쓰는 결과.
    fn passthrough(diff: &str) -> Self {
        Self {
            original_bytes: diff.len(),
            text: diff.to_string(),
            notes: Vec::new(),
        }
    }

    /// 접힌 내역을 프롬프트 꼬리에 붙일 한 줄. 무손실이면 None.
    pub fn footer(&self) -> Option<String> {
        if self.notes.is_empty() {
            return None;
        }
        Some(format!(
            "(diff 축약: {} — 원본 {} bytes. 전체는 작업 worktree에서 git diff로 확인하라.)",
            self.notes.join("; "),
            self.original_bytes
        ))
    }
}

/// unified diff처럼 보이는지 판별. 계획·산문에는 압축을 걸지 않기 위한 게이트다.
pub fn looks_like_unified_diff(s: &str) -> bool {
    let mut has_header = false;
    let mut has_hunk = false;
    for line in s.lines() {
        if line.starts_with("diff --git ") || line.starts_with("--- ") {
            has_header = true;
        } else if line.starts_with("@@") {
            has_hunk = true;
        }
        if has_header && has_hunk {
            return true;
        }
    }
    false
}

/// diff를 `budget` 바이트 안으로 줄인다. diff가 아니거나 이미 예산 이내면 그대로 통과.
pub fn compress_unified_diff(diff: &str, budget: usize) -> CompressedDiff {
    if diff.len() <= budget || !looks_like_unified_diff(diff) {
        return CompressedDiff::passthrough(diff);
    }

    let files = split_files(diff);
    let mut notes = Vec::new();

    // 1단계: 락파일 요약.
    let mut staged: Vec<String> = Vec::with_capacity(files.len());
    let mut locks_folded = 0usize;
    for file in &files {
        if is_lockfile(&file.path) {
            staged.push(summarize_lockfile(file));
            locks_folded += 1;
        } else {
            staged.push(file.body.clone());
        }
    }
    if locks_folded > 0 {
        notes.push(format!("잠금 파일 {locks_folded}개를 증감 요약으로 접음"));
    }

    // 2단계: 아직 넘치면 파일별 균등 배분.
    if joined_len(&staged) > budget {
        let share = (budget / staged.len().max(1)).max(MIN_FILE_BUDGET);
        let mut trimmed_files = 0usize;
        for slot in staged.iter_mut() {
            if slot.len() > share {
                *slot = trim_to_hunk_boundary(slot, share);
                trimmed_files += 1;
            }
        }
        if trimmed_files > 0 {
            notes.push(format!(
                "파일 {trimmed_files}개를 파일당 약 {share} bytes로 잘라냄"
            ));
        }
    }

    let text = staged.join("\n");
    // 압축이 손해를 내면 원본을 쓴다.
    if text.len() >= diff.len() {
        return CompressedDiff::passthrough(diff);
    }
    CompressedDiff {
        text,
        original_bytes: diff.len(),
        notes,
    }
}

/// 파일 하나 몫의 diff.
struct FileChunk {
    path: String,
    body: String,
}

/// `\n`으로 이은 길이(join 결과 길이와 동일).
fn joined_len(parts: &[String]) -> usize {
    let sum: usize = parts.iter().map(String::len).sum();
    sum + parts.len().saturating_sub(1)
}

/// `diff --git` 경계로 파일별 분할. 헤더가 없으면 전체를 한 덩이로 본다.
fn split_files(diff: &str) -> Vec<FileChunk> {
    let mut chunks: Vec<FileChunk> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some((path, lines)) = current.take() {
                chunks.push(FileChunk {
                    path,
                    body: lines.join("\n"),
                });
            }
            current = Some((path_from_git_header(line), vec![line]));
        } else if let Some((path, lines)) = current.as_mut() {
            // 헤더에서 경로를 못 얻었으면 `+++ b/...`에서 보완한다.
            if path.is_empty() {
                if let Some(rest) = line.strip_prefix("+++ ") {
                    *path = strip_diff_prefix(rest.trim());
                }
            }
            lines.push(line);
        } else {
            current = Some((String::new(), vec![line]));
        }
    }
    if let Some((path, lines)) = current {
        chunks.push(FileChunk {
            path,
            body: lines.join("\n"),
        });
    }
    chunks
}

/// `diff --git a/foo b/foo` → `foo`. 공백 포함 경로는 판별 불가이므로 빈 문자열(뒤에서 보완).
fn path_from_git_header(line: &str) -> String {
    let rest = line.trim_start_matches("diff --git ").trim();
    let fields: Vec<&str> = rest.split(' ').collect();
    if fields.len() == 2 {
        return strip_diff_prefix(fields[1]);
    }
    String::new()
}

/// `a/`·`b/` 접두와 따옴표 제거.
fn strip_diff_prefix(path: &str) -> String {
    let path = path.trim().trim_matches('"');
    for prefix in ["a/", "b/", "i/", "w/", "c/", "o/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    path.to_string()
}

/// 경로의 마지막 구간이 잠금 파일인지.
fn is_lockfile(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    LOCKFILES.contains(&name)
}

/// 잠금 파일을 헤더 + 증감 한 줄로 접는다.
fn summarize_lockfile(file: &FileChunk) -> String {
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut header = Vec::new();
    for line in file.body.lines() {
        if line.starts_with("@@") {
            // hunk 시작 — 여기부터는 세기만 한다.
            continue;
        }
        if let Some(rest) = line.strip_prefix('+') {
            if rest.starts_with("++ ") {
                header.push(line);
            } else {
                added += 1;
            }
        } else if let Some(rest) = line.strip_prefix('-') {
            if rest.starts_with("-- ") {
                header.push(line);
            } else {
                removed += 1;
            }
        } else if !line.starts_with(' ') && !line.starts_with('\\') {
            header.push(line);
        }
    }
    let label = if file.path.is_empty() {
        "잠금 파일".to_string()
    } else {
        file.path.clone()
    };
    format!(
        "{}\n@@ 잠금 파일 요약 @@\n(+{added}/-{removed} 줄 — 본문 생략: {label})",
        header.join("\n")
    )
}

/// `max` 바이트 안으로 자른다. 가능하면 hunk(`@@`) 경계에서 끊고, 생략 사실을 남긴다.
fn trim_to_hunk_boundary(body: &str, max: usize) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut used = 0usize;
    let mut dropped = 0usize;
    let mut full = true;
    for line in body.lines() {
        let cost = line.len() + 1;
        if full && used + cost <= max {
            kept.push(line);
            used += cost;
        } else {
            // 예산이 찬 뒤로는 전부 생략 — 줄 수만 센다.
            full = false;
            dropped += 1;
        }
    }
    if dropped == 0 {
        return kept.join("\n");
    }
    format!("{}\n… (이 파일에서 {dropped}줄 생략)", kept.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lockfile_diff(lines: usize) -> String {
        let mut s = String::from(
            "diff --git a/Cargo.lock b/Cargo.lock\n--- a/Cargo.lock\n+++ b/Cargo.lock\n@@ -1,4 +1,4 @@\n",
        );
        for i in 0..lines {
            s.push_str(&format!("+ version = \"1.0.{i}\"\n"));
        }
        s
    }

    fn source_diff(path: &str, body_lines: usize) -> String {
        let mut s =
            format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,3 +1,4 @@\n");
        for i in 0..body_lines {
            s.push_str(&format!("+let value_{i} = {i};\n"));
        }
        s
    }

    #[test]
    fn detects_unified_diff_only() {
        assert!(looks_like_unified_diff(
            "diff --git a/x b/x\n@@ -1 +1 @@\n-a\n+b"
        ));
        assert!(looks_like_unified_diff("--- a/x\n+++ b/x\n@@ -1 +1 @@\n+b"));
        assert!(
            !looks_like_unified_diff("구현 계획\n1. 모듈을 만든다\n2. 배선한다"),
            "산문은 diff가 아니다"
        );
        assert!(
            !looks_like_unified_diff("@@ 헤더만 있고 파일 헤더가 없다"),
            "hunk만으로는 diff로 보지 않는다"
        );
    }

    #[test]
    fn passes_through_when_within_budget() {
        let d = source_diff("src/a.rs", 3);
        let out = compress_unified_diff(&d, 64 * 1024);
        assert_eq!(out.text, d, "예산 이내면 그대로");
        assert!(out.notes.is_empty(), "무손실이면 note 없음");
        assert!(out.footer().is_none());
    }

    #[test]
    fn passes_through_non_diff_content() {
        let text = "계획 문서\n".repeat(5_000);
        let out = compress_unified_diff(&text, 1_000);
        assert_eq!(out.text, text, "diff가 아니면 건드리지 않는다");
    }

    #[test]
    fn folds_lockfile_and_keeps_source() {
        // 락파일이 앞(C), 소스가 뒤(s) — 기존 절단이 소스를 날리던 배치.
        let diff = format!("{}{}", lockfile_diff(4_000), source_diff("src/main.rs", 5));
        let out = compress_unified_diff(&diff, 8_000);
        assert!(
            out.text.contains("잠금 파일 요약"),
            "락파일은 요약으로 접힘"
        );
        assert!(
            !out.text.contains("version = \"1.0.3999\""),
            "락파일 본문은 빠짐"
        );
        assert!(
            out.text.contains("let value_4 = 4;"),
            "뒤쪽 소스 변경이 살아남아야 한다 — 이게 이 모듈의 존재 이유다"
        );
        assert!(out.text.len() < diff.len());
        assert!(out.footer().is_some(), "손실이 있으면 꼬리 고지");
    }

    #[test]
    fn every_file_survives_budget_pressure() {
        // 큰 소스 파일 3개 — 균등 배분되어 셋 다 등장해야 한다.
        let diff = format!(
            "{}{}{}",
            source_diff("src/a.rs", 800),
            source_diff("src/b.rs", 800),
            source_diff("src/c.rs", 800)
        );
        let out = compress_unified_diff(&diff, 4_000);
        for path in ["src/a.rs", "src/b.rs", "src/c.rs"] {
            assert!(
                out.text.contains(path),
                "{path}가 목록에서 사라지면 안 된다"
            );
        }
        assert!(out.text.contains("줄 생략"), "잘린 사실을 명시");
    }

    #[test]
    fn never_grows_beyond_original() {
        // 파일이 아주 많으면 최소 예산 합계가 원본을 넘을 수 있다 → 원본 통과로 되돌아야 한다.
        let mut diff = String::new();
        for i in 0..40 {
            diff.push_str(&source_diff(&format!("src/f{i}.rs"), 1));
        }
        let out = compress_unified_diff(&diff, 100);
        assert!(
            out.text.len() <= diff.len(),
            "압축이 원본보다 커지면 안 된다"
        );
    }

    #[test]
    fn handles_multibyte_paths_and_bodies() {
        let diff = format!(
            "diff --git a/문서/설계.md b/문서/설계.md\n--- a/문서/설계.md\n+++ b/문서/설계.md\n@@ -1 +1,2 @@\n{}",
            "+한글 본문 줄입니다\n".repeat(500)
        );
        let out = compress_unified_diff(&diff, 1_000);
        assert!(out.text.is_char_boundary(out.text.len()), "UTF-8 경계 보존");
        assert!(out.text.contains("문서/설계.md"));
        // 잘린 결과가 유효한 UTF-8 문자열인지 (String이므로 타입상 보장되지만 줄 단위 절단 확인).
        assert!(!out.text.contains('\u{FFFD}'), "치환 문자가 없어야 한다");
    }

    #[test]
    fn lockfile_detected_by_last_segment() {
        assert!(is_lockfile("Cargo.lock"));
        assert!(is_lockfile("app/pnpm-lock.yaml"));
        assert!(is_lockfile("deep/nested/go.sum"));
        assert!(!is_lockfile("src/lock.rs"), "이름이 다르면 락파일 아님");
        assert!(!is_lockfile("Cargo.toml"));
    }

    #[test]
    fn parses_path_from_headers() {
        assert_eq!(
            path_from_git_header("diff --git a/src/x.rs b/src/x.rs"),
            "src/x.rs"
        );
        assert_eq!(
            path_from_git_header("diff --git a/with space.rs b/with space.rs"),
            "",
            "공백 경로는 헤더로 판별 불가 — +++ 로 보완한다"
        );
        // 공백 경로도 +++ 보완 경로로 잡히는지.
        let diff = "diff --git a/with space.md b/with space.md\n--- a/with space.md\n+++ b/with space.md\n@@ -1 +1 @@\n+x";
        let files = split_files(diff);
        assert_eq!(files[0].path, "with space.md");
    }

    #[test]
    fn splits_files_without_git_header() {
        // `--- /+++` 만 있는 diff(no-index 등)도 한 덩이로 처리되어야 한다.
        let diff = "--- a/x.rs\n+++ b/x.rs\n@@ -1 +1 @@\n+a";
        let files = split_files(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "x.rs", "+++ 에서 경로 보완");
    }
}
