//! Markdown 청킹 — 검색 단위를 만든다.
//!
//! 목표 크기는 **문자 기준 근사**다. 토크나이저를 붙이면 의존성이 늘고, 여기서 필요한 건
//! 정확한 토큰 수가 아니라 "너무 크지 않은 조각"이다.
//!
//! `TARGET_CHARS`/`OVERLAP_CHARS`는 **임의 초기값**이다 — 한국어 문자당 토큰 비율을 감안한
//! 어림(1,200자 ≈ 512~800토큰)일 뿐 근거 있는 값이 아니다. 골든 쿼리 recall 측정으로
//! 조정한다. 측정 없이 고른 값은 다른 임의 값과 다르지 않다.

/// 청크 목표 길이(문자).
pub const TARGET_CHARS: usize = 1_200;
/// 인접 청크가 겹치는 길이(문자). 경계에 걸친 문장이 어느 쪽에서도 안 잡히는 것을 막는다.
pub const OVERLAP_CHARS: usize = 150;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub ord: i64,
    /// `설계 > 검색`처럼 상위 heading을 이어 붙인 경로. 결과에 "어디서 왔는지" 표시한다.
    pub heading: Option<String>,
    pub content: String,
}

/// Markdown을 청크로 나눈다. heading 경계를 우선하고, 코드펜스 안에서는 자르지 않는다.
pub fn split_markdown(text: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut buf = String::new();
    let mut path: Vec<String> = Vec::new();
    let mut buf_heading: Option<String> = None;
    let mut in_fence = false;

    for line in text.lines() {
        let fence = is_fence(line);
        if fence {
            in_fence = !in_fence;
        }

        // heading은 코드펜스 밖에서만 유효하다. 펜스 안의 `# 주석`을 heading으로 오인하면
        // 코드 블록이 쪼개진다.
        if !in_fence && !fence {
            if let Some((depth, title)) = parse_heading(line) {
                if !buf.trim().is_empty() {
                    push_chunk(&mut chunks, &mut buf, buf_heading.take());
                }
                path.truncate(depth.saturating_sub(1));
                path.push(title);
                buf_heading = Some(path.join(" > "));
                continue;
            }
        }

        buf.push_str(line);
        buf.push('\n');

        // 펜스 안에서는 크기를 넘겨도 자르지 않는다 — 반쪽 코드 블록은 검색 결과에서
        // 문법이 깨진 조각으로 보인다.
        if in_fence {
            continue;
        }

        // `while`인 이유: 개행 없는 긴 문단 하나가 목표의 열 배일 수 있다.
        // 라인 단위로만 자르면 그런 노트는 통째로 한 청크가 되어 임베딩 입력 길이를
        // 넘기고 뒷부분이 조용히 잘린다.
        while buf.chars().count() >= TARGET_CHARS {
            let head: String = buf.chars().take(TARGET_CHARS).collect();
            let rest: String = buf.chars().skip(TARGET_CHARS).collect();
            // 코드펜스가 방금 닫혔으면 overlap을 두지 않는다 — 꼬리에 닫는 ```가 섞여
            // 다음 청크가 반쪽 펜스로 시작한다. 블록 경계는 그 자체로 자연스러운 분할점이다.
            let carry = if fence { String::new() } else { tail(&head, OVERLAP_CHARS) };
            buf = head;
            push_chunk(&mut chunks, &mut buf, buf_heading.clone());
            buf = carry + &rest;
        }
    }

    if !buf.trim().is_empty() {
        push_chunk(&mut chunks, &mut buf, buf_heading);
    }
    chunks
}

fn push_chunk(chunks: &mut Vec<Chunk>, buf: &mut String, heading: Option<String>) {
    let content = buf.trim_end().to_string();
    buf.clear();
    if content.trim().is_empty() {
        return;
    }
    chunks.push(Chunk {
        ord: chunks.len() as i64,
        heading,
        content,
    });
}

/// 뒤에서 `n`자 — 문자 경계로 자른다(바이트로 자르면 한글에서 패닉).
fn tail(s: &str, n: usize) -> String {
    let total = s.chars().count();
    s.chars().skip(total.saturating_sub(n)).collect()
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// `## 제목` → `(2, "제목")`. heading이 아니면 None.
fn parse_heading(line: &str) -> Option<(usize, String)> {
    let depth = line.chars().take_while(|c| *c == '#').count();
    if depth == 0 || depth > 6 {
        return None;
    }
    let rest = line[depth..].trim();
    // `#태그`처럼 공백 없이 붙은 것은 heading이 아니다 — Obsidian 태그와 충돌한다.
    if rest.is_empty() || !line[depth..].starts_with(char::is_whitespace) {
        return None;
    }
    Some((depth, rest.to_string()))
}
