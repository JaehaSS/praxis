//! 워크트리 전체 내용 검색 — Shift×2 "어디서나 검색"의 코드 스코프.
//!
//! `ignore` 크레이트(ripgrep의 워커)를 쓴다. `.gitignore`를 따라가므로 `node_modules`·
//! `target`이 자동으로 빠진다 — 그게 없으면 이 레포에서 결과가 수십만 건이 되어 쓸모가 없다.
//!
//! Tauri 비의존 — `cargo test`로 직접 검증된다.

use std::path::Path;

use serde::Serialize;

/// 한 번에 돌려주는 최대 매치 수. 넘으면 잘렸다고 **말한다**.
pub const MAX_MATCHES: usize = 200;
/// 이보다 큰 파일은 건너뛴다 — 미니파이된 번들 한 줄에서 매치 수천 개가 나온다.
const MAX_FILE_BYTES: u64 = 1_000_000;
/// 결과 줄의 표시 상한. 긴 줄이 UI를 밀어내지 않게 자르되 **버리지는 않는다**.
const MAX_LINE_CHARS: usize = 300;
/// 이보다 짧은 질의는 받지 않는다 — 한 글자로는 거의 모든 파일이 걸린다.
const MIN_QUERY_CHARS: usize = 2;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SearchMatch {
    /// 워크트리 기준 상대 경로.
    pub path: String,
    /// 1-기반 줄 번호 — 에디터가 그대로 쓴다.
    pub line: u32,
    /// 1-기반 열. 바이트가 아니라 **문자** 기준이다(한글이 섞인 줄에서 커서가 어긋나지 않게).
    pub column: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    /// 상한에 걸려 잘렸는가. **조용히 자르지 않는다** — 잘린 목록은 "이게 전부"로 읽힌다.
    pub truncated: bool,
    pub scanned_files: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub include_hidden: bool,
}

/// NUL 바이트가 있으면 바이너리로 본다 — git이 쓰는 것과 같은 휴리스틱.
/// 매치돼도 보여줄 수 없는 것을 결과에 넣지 않는다.
fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

/// 문자 기준으로 자른다 — 바이트로 자르면 UTF-8 경계가 깨진다.
fn clip(line: &str) -> String {
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_string();
    }
    let mut s: String = line.chars().take(MAX_LINE_CHARS).collect();
    s.push('…');
    s
}

pub fn search(root: &Path, query: &str, opts: &SearchOptions) -> anyhow::Result<SearchResult> {
    let needle = query.trim();
    // 빈 질의에 전 파일을 돌려주면 UI가 얼어붙는다. 아무것도 아닌 것은 아무것도 아니다.
    if needle.chars().count() < MIN_QUERY_CHARS {
        return Ok(SearchResult {
            matches: Vec::new(),
            truncated: false,
            scanned_files: 0,
        });
    }
    let folded = if opts.case_sensitive {
        needle.to_string()
    } else {
        needle.to_lowercase()
    };

    let mut matches = Vec::new();
    let mut truncated = false;
    let mut scanned = 0u32;

    let walker = ignore::WalkBuilder::new(root)
        .hidden(!opts.include_hidden)
        .git_ignore(true)
        .git_global(false)
        // **git 저장소가 아니어도 `.gitignore`를 존중한다.** 기본값(`require_git = true`)은
        // `.git`이 있을 때만 규칙을 적용하는데, Praxis는 git이 아닌 폴더에서도 직접 실행을
        // 지원하므로 그런 폴더에서 `node_modules`가 통째로 걸리게 된다.
        .require_git(false)
        .parents(false)
        .build();

    'files: for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        if looks_binary(&bytes) {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        scanned += 1;

        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .into_owned();

        for (i, line) in text.lines().enumerate() {
            let hay = if opts.case_sensitive {
                line.to_string()
            } else {
                line.to_lowercase()
            };
            let Some(at) = hay.find(&folded) else { continue };
            if matches.len() >= MAX_MATCHES {
                truncated = true;
                break 'files;
            }
            // 바이트 오프셋 → 문자 열. 한글이 앞에 있으면 둘이 다르다.
            let column = line[..at].chars().count() as u32 + 1;
            matches.push(SearchMatch {
                path: rel.clone(),
                line: i as u32 + 1,
                column,
                text: clip(line),
            });
        }
    }

    Ok(SearchResult {
        matches,
        truncated,
        scanned_files: scanned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let d = crate::testtmp::dir().join(format!("praxis-search-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn find(root: &Path, q: &str) -> SearchResult {
        search(root, q, &SearchOptions::default()).unwrap()
    }

    #[test]
    fn finds_a_literal_across_files() {
        let d = tmp("literal");
        write(&d, "a.ts", "const needle = 1;\nother\n");
        write(&d, "sub/b.rs", "// needle here\n");
        let r = find(&d, "needle");
        assert_eq!(r.matches.len(), 2);
        assert!(r.matches.iter().any(|m| m.path.contains("a.ts") && m.line == 1));
        assert!(r.matches.iter().any(|m| m.path.contains("b.rs")));
    }

    #[test]
    fn respects_gitignore() {
        // 이 검사가 없으면 이 레포에서 검색이 실질적으로 못 쓰게 된다.
        let d = tmp("ignore");
        write(&d, ".gitignore", "node_modules/\n");
        write(&d, "keep.ts", "needle\n");
        write(&d, "node_modules/pkg/index.js", "needle\n");
        let r = find(&d, "needle");
        assert_eq!(r.matches.len(), 1, "gitignore된 파일이 결과에 들어왔다");
        assert!(r.matches[0].path.contains("keep.ts"));
    }

    #[test]
    fn skips_binary_files() {
        let d = tmp("binary");
        write(&d, "t.txt", "needle\n");
        std::fs::write(d.join("blob.bin"), b"needle\x00\x01\x02").unwrap();
        let r = find(&d, "needle");
        assert_eq!(r.matches.len(), 1);
        assert!(!r.matches[0].path.contains("blob.bin"));
    }

    #[test]
    fn truncates_and_says_so() {
        let d = tmp("truncate");
        let body = "needle\n".repeat(MAX_MATCHES + 50);
        write(&d, "many.txt", &body);
        let r = find(&d, "needle");
        assert_eq!(r.matches.len(), MAX_MATCHES);
        assert!(r.truncated, "잘렸는데 말하지 않았다");
    }

    #[test]
    fn long_lines_are_cut_not_dropped() {
        let d = tmp("longline");
        write(&d, "min.js", &format!("{}needle{}", "x".repeat(5000), "y".repeat(5000)));
        let r = find(&d, "needle");
        assert_eq!(r.matches.len(), 1, "긴 줄의 매치가 사라졌다");
        assert!(r.matches[0].text.chars().count() <= MAX_LINE_CHARS + 1);
    }

    #[test]
    fn case_insensitive_by_default_and_sensitive_on_request() {
        let d = tmp("case");
        write(&d, "a.txt", "Needle\n");
        assert_eq!(find(&d, "needle").matches.len(), 1);
        let strict = search(
            &d,
            "needle",
            &SearchOptions {
                case_sensitive: true,
                include_hidden: false,
            },
        )
        .unwrap();
        assert_eq!(strict.matches.len(), 0);
    }

    #[test]
    fn a_short_query_returns_nothing_not_everything() {
        let d = tmp("short");
        write(&d, "a.txt", "aaaa\n");
        assert_eq!(find(&d, "").matches.len(), 0);
        assert_eq!(find(&d, "a").matches.len(), 0);
        assert_eq!(find(&d, "aa").matches.len(), 1);
    }

    #[test]
    fn column_is_counted_in_characters_not_bytes() {
        // 한글 뒤의 매치를 바이트로 세면 에디터 커서가 엉뚱한 자리에 선다.
        let d = tmp("column");
        write(&d, "ko.txt", "한글앞 needle\n");
        let r = find(&d, "needle");
        assert_eq!(r.matches[0].column, 5, "열을 바이트로 셌다");
    }
}
