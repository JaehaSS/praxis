//! Obsidian `[[wikilink]]` 파싱과 대상 해소.
//!
//! wikilink는 vault를 이미 그래프로 만들어 둔 것이라, 엣지 모델을 가장 싸게 검증한다
//! (설계 0020 DR-8).

use std::collections::HashMap;

/// 본문에서 링크 대상을 뽑는다. 별칭(`|`)·앵커(`#`)·블록참조(`^`)는 벗겨낸다.
/// 임베드(`![[...]]`)도 같은 링크로 취급한다 — 그래프상 참조인 것은 같다.
pub fn parse_wikilinks(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == '[' && bytes[i + 1] == '[' {
            if let Some(end) = find_close(&bytes, i + 2) {
                let inner: String = bytes[i + 2..end].iter().collect();
                if let Some(target) = normalize_target(&inner) {
                    if !out.contains(&target) {
                        out.push(target);
                    }
                }
                i = end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn find_close(chars: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == ']' && chars[i + 1] == ']' {
            return Some(i);
        }
        // 링크는 줄을 넘지 않는다. 닫는 괄호를 안 만나면 링크가 아니다.
        if chars[i] == '\n' {
            return None;
        }
        i += 1;
    }
    None
}

fn normalize_target(inner: &str) -> Option<String> {
    let head = inner.split('|').next()?; // 별칭 제거
    let head = head.split('#').next()?; // 앵커 제거
    let head = head.split('^').next()?; // 블록 참조 제거
    let trimmed = head.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// 링크 대상 → `external_id`.
///
/// Obsidian은 경로 없이 파일명만으로 링크한다(`[[노트]]`). 따라서 stem 색인이 필요하다.
/// 경로가 포함된 링크(`[[폴더/노트]]`)는 그대로 맞춘다.
pub struct TargetIndex {
    by_id: HashMap<String, i64>,
    by_stem: HashMap<String, Vec<i64>>,
}

impl TargetIndex {
    pub fn build(nodes: &[(i64, String)]) -> Self {
        let mut by_id = HashMap::new();
        let mut by_stem: HashMap<String, Vec<i64>> = HashMap::new();
        for (id, external_id) in nodes {
            by_id.insert(external_id.clone(), *id);
            let stem = external_id
                .rsplit('/')
                .next()
                .unwrap_or(external_id)
                .trim_end_matches(".md")
                .to_string();
            by_stem.entry(stem).or_default().push(*id);
        }
        Self { by_id, by_stem }
    }

    /// 대상 노드 id. 못 찾으면 None — **유령 노드를 만들지 않는다.**
    /// 아직 안 만든 노트로 링크하는 건 Obsidian에서 정상이고, 그때마다 빈 문서를
    /// 만들면 검색 결과가 빈 껍데기로 오염된다.
    pub fn resolve(&self, target: &str) -> Option<i64> {
        let with_ext = if target.ends_with(".md") {
            target.to_string()
        } else {
            format!("{target}.md")
        };
        if let Some(id) = self.by_id.get(&with_ext) {
            return Some(*id);
        }
        let stem = target.rsplit('/').next().unwrap_or(target);
        match self.by_stem.get(stem) {
            // 동명이인이면 해소하지 않는다. 임의로 하나를 고르면 그래프가 조용히 틀려진다.
            Some(ids) if ids.len() == 1 => Some(ids[0]),
            _ => None,
        }
    }
}
