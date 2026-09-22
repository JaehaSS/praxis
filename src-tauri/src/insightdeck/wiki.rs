//! 지식창고 문서 → 인사이트 카드.
//!
//! 덱 파일(`deck.rs`)은 "코드를 고치지 않고 내용을 더하는" 길이었지만, 규격 MD를 Finder에서
//! 손으로 쓰는 사람은 없었다(원장 #506 — 덱 폴더에 카드가 0장). 사용자가 이미 쓰고 있는
//! 글이 개인 지식창고에 있으므로 **그 문서를 그대로 카드로 자른다.** 문서 하나의 `## 절`
//! 하나가 카드 하나이고, 출처는 문서의 상대 경로다 — 카드마다 출처를 요구하는 규약
//! (ADR 0154)이 여기서는 파일 경로로 저절로 채워진다.
//!
//! 덱 이름은 최상위 폴더다(`업무-인프라/…` → `업무-인프라`). `pick_next`가 태그가 겹치지
//! 않는 카드를 먼저 고르므로 폴더명을 태그에도 넣어 폴더가 번갈아 나오게 한다.
//!
//! **범위(scope)** — 창고 전체가 리마인드 감은 아니다(작업일지의 README 절이 카드로 떴다).
//! 호출자가 폴더 목록을 넘기면 그 밑의 문서만 자른다. `None`은 전체, 빈 목록은 아무것도
//! 안 자른다 — "아직 안 골랐다"와 "다 껐다"는 다른 상태다. 범위 밖 폴더는 파싱하지 않고
//! 문서 수만 센다. 설정 화면이 고를 수 있게 폴더 목록은 범위와 무관하게 돌려준다.
//!
//! Tauri·DB 비의존 — `cargo test`로 직접 검증된다. 창고 루트를 찾는 일은 호출자가 한다.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::deck::{Deck, InsightCard};

/// 루트 바로 밑 문서의 덱 이름. 폴더가 없으니 창고 자체를 이름으로 쓴다.
pub const ROOT_DECK: &str = "지식창고";
/// 본문 상한(문자 수). 대기 카드는 스치듯 읽는 것이라 절 전체를 싣지 않는다.
pub const MAX_BODY_CHARS: usize = 500;
/// 이보다 짧은 절은 카드가 못 된다 — 제목만 있는 절, 링크 한 줄짜리 절을 거른다.
pub const MIN_BODY_CHARS: usize = 60;
/// 한 파일 상한. 창고에는 세션 내보내기 같은 거대 MD가 섞일 수 있다.
const MAX_NOTE_BYTES: u64 = 1024 * 1024;
/// 한 번에 읽을 문서 수 상한. 넘으면 경고를 남기고 멈춘다 — 대기 카드 한 장을 위해
/// 창고 전체를 매번 읽는 비용에 천장을 둔다.
const MAX_NOTES: usize = 2000;

/// 범위 목록에서 루트 바로 밑 문서를 가리키는 키. 폴더가 아니라 이름을 지을 수 없어
/// 파일 시스템의 관용(`.` = 여기)을 빌린다.
pub const ROOT_FOLDER: &str = ".";

/// 창고의 최상위 폴더 하나 — 설정 화면이 범위를 고르는 단위다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VaultFolder {
    /// 루트 기준 상대 경로. 루트 문서는 [`ROOT_FOLDER`].
    pub path: String,
    /// 이 폴더 밑 `.md` 수 — 범위 밖이어도 센다.
    pub notes: u32,
    /// 잘라 낸 카드 수. 범위 밖이면 파싱하지 않으므로 0이다.
    pub cards: u32,
    /// 이 폴더의 문서가 하나라도 범위에 들었는가.
    pub included: bool,
}

#[derive(Debug, Default)]
pub struct VaultCards {
    pub decks: Vec<Deck>,
    /// 범위 안에서 읽은 문서 수(카드가 안 나온 문서 포함).
    pub notes: u32,
    /// 최상위 폴더 목록(경로순). 범위 밖 폴더도 들어 있다.
    pub folders: Vec<VaultFolder>,
    pub warnings: Vec<String>,
}

impl VaultCards {
    pub fn cards(&self) -> u32 {
        self.decks.iter().map(|d| d.cards.len() as u32).sum()
    }
}

/// 상대 경로가 범위에 드는가. `None`은 전체, 빈 목록은 아무것도 아니다.
/// 항목은 폴더 경로라 접두사 + `/`로 맞춘다 — `개인`이 `개인정보/…`를 삼키지 않는다.
pub fn in_scope(relative: &str, scope: Option<&[String]>) -> bool {
    let Some(scope) = scope else {
        return true;
    };
    scope.iter().any(|folder| {
        let folder = folder.trim_end_matches('/');
        if folder == ROOT_FOLDER {
            !relative.contains('/')
        } else {
            !folder.is_empty()
                && relative.len() > folder.len()
                && relative.starts_with(folder)
                && relative.as_bytes()[folder.len()] == b'/'
        }
    })
}

/// 상대 경로의 최상위 폴더 키 — [`deck_name`]과 같되 루트 문서는 [`ROOT_FOLDER`]다.
pub fn folder_key(relative: &str) -> String {
    match relative.split_once('/') {
        Some((folder, _)) if !folder.is_empty() => folder.to_string(),
        _ => ROOT_FOLDER.to_string(),
    }
}

/// 창고 루트 밑의 `.md`를 걸어 카드로 자른다. 루트가 없으면 빈 결과다.
///
/// 건너뛰는 것: 숨김 폴더·파일(`.git`, `.knowledge`, `.obsidian` …), 심볼릭 링크(창고 밖으로
/// 따라가지 않는다 — 창고 계약과 같다), 루트의 에이전트 지침 파일, 1 MiB 넘는 파일.
///
/// `scope`는 [`in_scope`]의 의미다. 범위 밖 문서는 읽지 않고 폴더 통계에만 센다.
pub fn load_vault_cards(root: &Path, scope: Option<&[String]>) -> VaultCards {
    let mut out = VaultCards::default();
    if !root.is_dir() {
        return out;
    }
    let mut files = Vec::new();
    collect_notes(root, root, &mut files, &mut out.warnings);
    files.sort();
    if files.len() > MAX_NOTES {
        out.warnings.push(format!(
            "지식창고 문서가 {}편이라 앞의 {MAX_NOTES}편만 카드로 읽었습니다.",
            files.len()
        ));
        files.truncate(MAX_NOTES);
    }
    for path in files {
        let relative = match path.strip_prefix(root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        let folder = {
            let key = folder_key(&relative);
            match out.folders.iter().position(|f| f.path == key) {
                Some(i) => i,
                None => {
                    out.folders.push(VaultFolder {
                        path: key,
                        notes: 0,
                        cards: 0,
                        included: false,
                    });
                    out.folders.len() - 1
                }
            }
        };
        out.folders[folder].notes += 1;
        if !in_scope(&relative, scope) {
            continue;
        }
        out.folders[folder].included = true;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                out.warnings.push(format!("{relative}: 읽지 못했습니다 ({e})"));
                continue;
            }
        };
        out.notes += 1;
        let cards = note_cards(&relative, &text);
        if cards.is_empty() {
            continue;
        }
        out.folders[folder].cards += cards.len() as u32;
        let deck_name = deck_name(&relative);
        match out.decks.iter_mut().find(|d| d.name == deck_name) {
            Some(deck) => deck.cards.extend(cards),
            None => out.decks.push(Deck {
                name: deck_name,
                cards,
            }),
        }
    }
    // 파일은 정렬돼 있지만 루트 문서(`.`)는 폴더와 섞여 들어오므로 여기서 한 번 더 고정한다.
    out.folders.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn collect_notes(root: &Path, dir: &Path, files: &mut Vec<PathBuf>, warnings: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            warnings.push(format!("{}: 폴더를 읽지 못했습니다 ({e})", dir.display()));
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        // 링크는 대상이 창고 안이든 밖이든 따라가지 않는다 — 창고 파일 계약과 같다.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect_notes(root, &path, files, warnings);
            continue;
        }
        if !meta.is_file() || !name.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        if dir == root && matches!(name, "CLAUDE.md" | "AGENTS.md" | "GEMINI.md") {
            continue;
        }
        if meta.len() > MAX_NOTE_BYTES {
            continue;
        }
        files.push(path);
    }
}

/// 상대 경로의 최상위 폴더가 덱이다. 루트 파일은 [`ROOT_DECK`].
pub fn deck_name(relative: &str) -> String {
    match relative.split_once('/') {
        Some((folder, _)) if !folder.is_empty() => folder.to_string(),
        _ => ROOT_DECK.to_string(),
    }
}

/// 문서 하나를 카드들로 자른다. `relative`는 창고 루트 기준 경로(출처·키에 쓴다).
///
/// - 제목: frontmatter `title` → 첫 `# ` 줄 → 파일 이름.
/// - 첫 `## ` 앞의 머리말이 충분히 길면 그것도 한 장(제목이 곧 카드 제목).
/// - `## 절`마다 한 장. `###` 이하는 절 본문에 녹인다.
/// - 본문에서 코드 펜스·표·이미지·필드 줄(`- **k**: v`)을 뺀다. 링크는 글자만 남긴다.
pub fn note_cards(relative: &str, text: &str) -> Vec<InsightCard> {
    let (front, body) = split_frontmatter(text);
    let stem = Path::new(relative)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(relative)
        .to_string();
    let deck = deck_name(relative);

    let mut title: Option<String> = front.title.clone();
    let mut preamble: Vec<&str> = Vec::new();
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    let mut in_fence = false;
    for line in body.lines() {
        let fence = is_fence(line);
        if fence {
            in_fence = !in_fence;
        }
        if !in_fence && !fence {
            if let Some(h) = line.strip_prefix("# ") {
                if title.is_none() && sections.is_empty() {
                    title = Some(clean_inline(h));
                    continue;
                }
            }
            if let Some(h) = line.strip_prefix("## ") {
                sections.push((clean_inline(h), Vec::new()));
                continue;
            }
        }
        match sections.last_mut() {
            Some((_, lines)) => lines.push(line),
            None => preamble.push(line),
        }
    }
    let title = title.filter(|t| !t.is_empty()).unwrap_or(stem);

    let mut tags: Vec<String> = front.tags.clone();
    if !tags.iter().any(|t| t == &deck) {
        tags.push(deck.clone());
    }

    let mut cards = Vec::new();
    let mut push = |heading: &str, lines: &[&str], with_heading: bool| {
        let body = clean_body(lines);
        if body.chars().count() < MIN_BODY_CHARS {
            return;
        }
        let source = if with_heading {
            format!("{relative} › {heading}")
        } else {
            relative.to_string()
        };
        cards.push(InsightCard {
            key: format!("wiki:{relative}#{heading}"),
            deck: deck.clone(),
            title: heading.to_string(),
            body: truncate(&body, MAX_BODY_CHARS),
            source,
            tags: tags.clone(),
        });
    };
    push(&title, &preamble, false);
    for (heading, lines) in &sections {
        if heading.is_empty() {
            continue;
        }
        push(heading, lines, true);
    }
    cards
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Frontmatter {
    title: Option<String>,
    tags: Vec<String>,
}

/// 파일 앞의 `---` 블록에서 `title`·`tags`만 읽고 나머지 본문을 돌려준다.
/// 블록이 닫히지 않으면 frontmatter가 없는 것으로 본다.
fn split_frontmatter(text: &str) -> (Frontmatter, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return (Frontmatter::default(), text);
    };
    let Some(end) = rest.find("\n---") else {
        return (Frontmatter::default(), text);
    };
    let mut front = Frontmatter::default();
    let mut in_tags = false;
    for line in rest[..end].lines() {
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            if in_tags {
                let tag = unquote(item);
                if !tag.is_empty() {
                    front.tags.push(tag);
                }
            }
            continue;
        }
        in_tags = false;
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "title" => front.title = Some(unquote(value)).filter(|t| !t.is_empty()),
            "tags" => {
                if let Some(inline) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                    front
                        .tags
                        .extend(inline.split(',').map(unquote).filter(|t| !t.is_empty()));
                } else if value.is_empty() {
                    in_tags = true;
                } else {
                    front.tags.push(unquote(value));
                }
            }
            _ => {}
        }
    }
    let body = &rest[end + "\n---".len()..];
    (front, body.strip_prefix('\n').unwrap_or(body))
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    let v = v
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(v);
    v.trim().to_string()
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// 절의 줄들을 한 문단으로 접는다. 카드는 서식이 아니라 글을 보여 주는 자리다.
fn clean_body(lines: &[&str]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut in_fence = false;
    for raw in lines {
        if is_fence(raw) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with('|') || line.starts_with('#') {
            continue;
        }
        // 표 구분선·수평선·이미지 줄.
        if line.chars().all(|c| matches!(c, '-' | '=' | '*' | '_' | ' ')) || line.starts_with("![") {
            continue;
        }
        let line = line.strip_prefix("> ").unwrap_or(line);
        let line = strip_list_marker(line);
        // `- **key**: value` 필드 줄은 메타데이터다 — 덱 파일 규약과 같은 이유로 뺀다.
        if line.starts_with("**") && line.contains("**:") {
            continue;
        }
        let cleaned = clean_inline(line);
        if !cleaned.is_empty() {
            parts.push(cleaned);
        }
    }
    parts.join(" ")
}

fn strip_list_marker(line: &str) -> &str {
    if let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
    {
        return rest.trim_start();
    }
    // `1. ` / `12) `
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return rest.trim_start();
        }
    }
    line
}

/// 인라인 서식을 벗긴다 — `[[link|alias]]`·`[text](url)`은 글자만, `**`·`` ` ``는 뗀다.
fn clean_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("[[") {
            if let Some(end) = after.find("]]") {
                let inner = &after[..end];
                let shown = inner.rsplit_once('|').map(|(_, a)| a).unwrap_or(inner);
                out.push_str(shown.split('#').next().unwrap_or(shown));
                rest = &after[end + 2..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('[') {
            if let Some(close) = after.find("](") {
                if let Some(end) = after[close..].find(')') {
                    out.push_str(&after[..close]);
                    rest = &after[close + end + 1..];
                    continue;
                }
            }
        }
        if let Some(after) = rest
            .strip_prefix("**")
            .or_else(|| rest.strip_prefix("__"))
        {
            rest = after;
            continue;
        }
        if let Some(after) = rest.strip_prefix('`') {
            rest = after;
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max).collect();
    // 낱말 중간에서 자르지 않는다 — 마지막 공백까지 물린다.
    if let Some(space) = cut.rfind(' ') {
        if space > max / 2 {
            cut.truncate(space);
        }
    }
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "---\ntitle: \"Vault 개요\"\ntags: [infra, k8s]\naliases: [vault]\n---\n\n# 이 줄은 title이 있으니 본문\n\n머리말은 짧아서 카드가 안 된다.\n\n## 왜 쓰는가\n\n비밀 값을 코드나 환경 변수에 두지 않고 한곳에서 **발급·회수**한다. [[운영-정책|정책]]과 [문서](https://x)를 따른다. 동적 시크릿은 만료가 있어 유출 창이 짧다.\n\n```sh\nvault kv get secret/db\n```\n\n| 항목 | 값 |\n|---|---|\n| 포트 | 8200 |\n\n### 세부\n\n- 항목 하나는 리스트다\n- **source**: 이건 필드 줄\n\n## 너무 짧은 절\n\n한 줄.\n";

    #[test]
    fn splits_sections_into_cards_with_path_source() {
        let cards = note_cards("업무-인프라/vault.md", NOTE);
        assert_eq!(cards.len(), 1, "{cards:#?}");
        let c = &cards[0];
        assert_eq!(c.key, "wiki:업무-인프라/vault.md#왜 쓰는가");
        assert_eq!(c.deck, "업무-인프라");
        assert_eq!(c.title, "왜 쓰는가");
        assert_eq!(c.source, "업무-인프라/vault.md › 왜 쓰는가");
        assert_eq!(c.tags, vec!["infra", "k8s", "업무-인프라"]);
        assert!(c.body.starts_with("비밀 값을 코드나 환경 변수에 두지 않고 한곳에서 발급·회수한다. 정책과 문서를 따른다."), "{}", c.body);
        assert!(c.body.contains("항목 하나는 리스트다"));
        assert!(!c.body.contains("vault kv get"), "코드 펜스는 뺀다");
        assert!(!c.body.contains("8200"), "표는 뺀다");
        assert!(!c.body.contains("이건 필드 줄"), "필드 줄은 뺀다");
        assert!(!c.body.contains("세부"), "하위 제목은 뺀다");
    }

    #[test]
    fn preamble_becomes_card_titled_by_note() {
        let long = "가".repeat(80);
        let text = format!("# 제목 줄\n\n{long}\n\n## 절\n\n{long}\n");
        let cards = note_cards("메모.md", &text);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].title, "제목 줄");
        assert_eq!(cards[0].source, "메모.md");
        assert_eq!(cards[0].deck, ROOT_DECK);
        assert_eq!(cards[0].tags, vec![ROOT_DECK]);
        assert_eq!(cards[1].key, "wiki:메모.md#절");
    }

    #[test]
    fn falls_back_to_file_stem_and_truncates_body() {
        let word = "낱말 ";
        let text = format!("## 긴 절\n\n{}", word.repeat(300));
        let cards = note_cards("문서/학습자료/긴-글.md", &text);
        assert_eq!(cards.len(), 1);
        assert!(cards[0].body.chars().count() <= MAX_BODY_CHARS + 1);
        assert!(cards[0].body.ends_with('…'));
        // 제목 없는 문서의 머리말 카드는 stem 제목을 쓴다.
        let text2 = "글 ".repeat(100);
        let cards2 = note_cards("문서/학습자료/긴-글.md", &text2);
        assert_eq!(cards2[0].title, "긴-글");
    }

    #[test]
    fn frontmatter_list_tags_and_unclosed_block() {
        let text = "---\ntags:\n  - a\n  - b\n---\n## 절\n\n".to_string() + &"내용 ".repeat(40);
        let cards = note_cards("x.md", &text);
        assert_eq!(cards[0].tags, vec!["a", "b", ROOT_DECK]);
        // 닫히지 않은 블록은 본문이다 — 카드가 나오지 않아도 패닉하지 않는다.
        let cards = note_cards("y.md", "---\ntitle: x\n## 절\n내용");
        assert!(cards.is_empty());
    }

    #[test]
    fn heading_inside_fence_is_not_a_section() {
        let long = "본문 ".repeat(40);
        let text = format!("## 진짜 절\n\n{long}\n```md\n## 가짜 절\n```\n");
        let cards = note_cards("x.md", &text);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "진짜 절");
    }

    /// 실제 창고에서 카드가 어떻게 나오는지 본다 — 파서를 손볼 때 규격 예문 대신 내 글로 확인하는 용도.
    /// `PRAXIS_INSIGHT_VAULT=/path/to/vault cargo test --lib dump_real_vault -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_real_vault() {
        let Ok(root) = std::env::var("PRAXIS_INSIGHT_VAULT") else {
            return;
        };
        let out = load_vault_cards(Path::new(&root), None);
        println!("notes={} cards={} warnings={:?}", out.notes, out.cards(), out.warnings);
        for deck in &out.decks {
            println!("[{}] {}장", deck.name, deck.cards.len());
            for card in &deck.cards {
                println!("  · {} ({}자) ← {}", card.title, card.body.chars().count(), card.source);
            }
        }
    }

    #[test]
    fn walks_vault_and_skips_hidden_links_and_agent_files() {
        let root = crate::testtmp::dir().join(format!("insight-wiki-{}", line!()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("업무-인프라")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join(".knowledge")).unwrap();
        let long = "본문 ".repeat(40);
        std::fs::write(root.join("업무-인프라/a.md"), format!("## 절\n\n{long}")).unwrap();
        std::fs::write(root.join("업무-인프라/b.md"), format!("## 절\n\n{long}\n\n## 둘\n\n{long}")).unwrap();
        std::fs::write(root.join("위키-시작.md"), format!("## 시작\n\n{long}")).unwrap();
        std::fs::write(root.join(".git/x.md"), format!("## 숨김\n\n{long}")).unwrap();
        std::fs::write(root.join(".knowledge/y.md"), format!("## 숨김\n\n{long}")).unwrap();
        std::fs::write(root.join("GEMINI.md"), format!("## 지침\n\n{long}")).unwrap();
        std::fs::write(root.join("그래프.json"), "{}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("업무-인프라"), root.join("링크")).unwrap();

        let out = load_vault_cards(&root, None);
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        assert_eq!(out.notes, 3);
        assert_eq!(out.cards(), 4);
        let folders: Vec<(&str, u32, u32, bool)> = out
            .folders
            .iter()
            .map(|f| (f.path.as_str(), f.notes, f.cards, f.included))
            .collect();
        assert_eq!(folders, vec![(ROOT_FOLDER, 1, 1, true), ("업무-인프라", 2, 3, true)]);
        let mut names: Vec<&str> = out.decks.iter().map(|d| d.name.as_str()).collect();
        names.sort();
        // 기대값도 같이 정렬한다 — 픽스처 폴더명이 바뀌면 ROOT_DECK과의 앞뒤가 뒤집힌다.
        let mut expected = vec![ROOT_DECK, "업무-인프라"];
        expected.sort();
        assert_eq!(names, expected);
        let infra = out.decks.iter().find(|d| d.name == "업무-인프라").unwrap();
        assert_eq!(infra.cards.len(), 3);
        assert!(infra.cards.iter().all(|c| c.key.starts_with("wiki:업무-인프라/")));

        assert_eq!(load_vault_cards(&root.join("없음"), None).cards(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scope_limits_parsing_but_still_lists_every_folder() {
        let root = crate::testtmp::dir().join(format!("insight-wiki-{}", line!()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("개인/학습")).unwrap();
        std::fs::create_dir_all(root.join("개인정보")).unwrap();
        std::fs::create_dir_all(root.join("업무-일지")).unwrap();
        let long = "본문 ".repeat(40);
        std::fs::write(root.join("개인/학습/a.md"), format!("## 하나

{long}")).unwrap();
        std::fs::write(root.join("개인/b.md"), format!("## 둘

{long}")).unwrap();
        std::fs::write(root.join("개인정보/c.md"), format!("## 셋

{long}")).unwrap();
        std::fs::write(root.join("업무-일지/d.md"), format!("## 넷

{long}")).unwrap();
        std::fs::write(root.join("루트.md"), format!("## 다섯

{long}")).unwrap();

        // 접두사가 아니라 폴더 경계로 맞춘다 — `개인`이 `개인정보`를 삼키지 않는다.
        let scope = vec!["개인".to_string()];
        let out = load_vault_cards(&root, Some(&scope));
        assert_eq!(out.notes, 2);
        assert_eq!(out.cards(), 2);
        assert_eq!(out.decks.len(), 1);
        assert_eq!(out.decks[0].name, "개인");
        let folders: Vec<(&str, u32, u32, bool)> = out
            .folders
            .iter()
            .map(|f| (f.path.as_str(), f.notes, f.cards, f.included))
            .collect();
        assert_eq!(
            folders,
            vec![
                (ROOT_FOLDER, 1, 0, false),
                ("개인", 2, 2, true),
                ("개인정보", 1, 0, false),
                ("업무-일지", 1, 0, false),
            ]
        );

        // 하위 경로도 그대로 받는다. 상위 폴더는 "일부가 들었다"로 보고된다.
        let nested = vec!["개인/학습".to_string(), ROOT_FOLDER.to_string()];
        let out = load_vault_cards(&root, Some(&nested));
        assert_eq!(out.cards(), 2);
        let keys: Vec<&str> = out.decks.iter().flat_map(|d| d.cards.iter()).map(|c| c.key.as_str()).collect();
        assert_eq!(keys, vec!["wiki:개인/학습/a.md#하나", "wiki:루트.md#다섯"]);
        assert!(out.folders.iter().find(|f| f.path == "개인").unwrap().included);

        // 빈 목록은 전체가 아니라 아무것도 아니다.
        let none: Vec<String> = Vec::new();
        let out = load_vault_cards(&root, Some(&none));
        assert_eq!(out.cards(), 0);
        assert_eq!(out.notes, 0);
        assert_eq!(out.folders.len(), 4);
        assert!(out.folders.iter().all(|f| !f.included));
        let _ = std::fs::remove_dir_all(&root);
    }
}
