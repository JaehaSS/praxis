//! 원문 → 색인 가능한 본문 (설계 0020 §7 계층표 / 플랜 0028 Task 5).
//!
//! 메일 본문의 상당 부분은 **인용문과 서명**이다. 그대로 색인하면 같은 대화의 모든
//! 메일이 서로의 사본을 품어, 검색 결과가 한 스레드로 도배된다. 반대로 너무 세게
//! 지우면 정작 사람이 쓴 한 줄까지 사라진다 — 그래서 마지막에 안전망을 둔다.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

use super::source::gmail_api::MessagePart;

/// MIME 트리에서 색인할 본문을 고른다.
///
/// `text/plain` 우선. 없을 때만 `text/html`을 텍스트로 낮춘다 — HTML은 태그를 걷어내도
/// 레이아웃 잔재가 남아 노이즈가 되므로 차선이다.
pub fn extract_body(payload: &MessagePart) -> String {
    if let Some(plain) = find_part(payload, "text/plain") {
        return plain;
    }
    find_part(payload, "text/html")
        .map(|html| strip_html(&html))
        .unwrap_or_default()
}

fn find_part(part: &MessagePart, want: &str) -> Option<String> {
    if part.mime_type.starts_with(want) {
        if let Some(text) = part.body.as_ref().and_then(|b| b.data.as_ref()) {
            if let Some(decoded) = decode_base64url(text) {
                return Some(decoded);
            }
        }
    }
    // 깊이 우선. multipart/alternative는 plain과 html을 형제로 두므로
    // 첫 일치를 그대로 쓰면 된다.
    part.parts.iter().find_map(|child| find_part(child, want))
}

/// Gmail은 base64**url**을 쓴다(`-`/`_`). 표준 base64로 디코드하면 조용히 실패한다.
/// 패딩은 붙어 올 때도 있어 벗겨 낸다.
fn decode_base64url(data: &str) -> Option<String> {
    let cleaned: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    let cleaned = cleaned.trim_end_matches('=');
    let bytes = URL_SAFE_NO_PAD.decode(cleaned).ok()?;
    // 메일 인코딩은 제각각이다. 깨진 바이트 때문에 메일 하나를 통째로 잃지 않도록
    // 손실 변환을 쓴다.
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// 태그를 걷어내고 엔티티 몇 개만 되돌린다. 완전한 HTML 파서를 들이지 않는 이유는
/// 본문 추출에 그만한 정확도가 필요 없고, 의존성만 늘기 때문이다.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

/// 인용문·전달문·서명을 잘라 낸다.
///
/// **잘라 낸 결과가 비면 원문을 돌려준다.** 인용만으로 이뤄진 메일("동의합니다" 없이
/// 전달만 한 경우)에서 색인이 통째로 사라지는 것을 막는 안전망이다. 노이즈가 조금
/// 남는 것보다 문서가 사라지는 쪽이 훨씬 나쁘다.
pub fn strip_quoted(body: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for line in body.lines() {
        if is_quote_boundary(line) {
            break;
        }
        if line.trim_start().starts_with('>') {
            continue;
        }
        kept.push(line);
    }

    let trimmed = kept.join("\n").trim().to_string();
    if trimmed.is_empty() {
        return body.trim().to_string();
    }
    trimmed
}

/// 이 줄부터 아래는 전부 인용이다.
fn is_quote_boundary(line: &str) -> bool {
    let t = line.trim();
    // 서명 구분자. RFC 3676이 정한 `-- `이고, 뒤 공백이 없는 변형도 흔하다.
    if t == "--" || line == "-- " {
        return true;
    }
    if t.starts_with("----------") && t.contains("essage") {
        return true; // ---------- Forwarded message ----------
    }
    if t.starts_with("_____") {
        return true; // Outlook이 넣는 구분선
    }
    // "On <날짜>, <사람> wrote:" — 줄바꿈으로 쪼개지는 경우가 많아 접두/접미로만 본다.
    if t.starts_with("On ") && t.ends_with("wrote:") {
        return true;
    }
    // 한국어 Gmail: "2026년 8월 6일 (수) 오전 9:00, 홍길동 <a@b.c>님이 작성:"
    if t.ends_with("님이 작성:") || t.ends_with("님이 작성했습니다:") {
        return true;
    }
    // 헤더 블록형 인용 (Outlook 등)
    if t.starts_with("From:") && line.starts_with("From:") {
        return true;
    }
    false
}

/// 제목과 본문을 색인용 문서 본문으로 합친다.
///
/// 제목을 본문 앞에 한 번 더 넣지 않는다 — `doc_title`이 청크마다 복제되어 이미
/// 색인되기 때문이다(#200). 여기서 또 넣으면 같은 문자열이 두 번 매칭돼 순위가 왜곡된다.
pub fn message_body(payload: &MessagePart) -> String {
    strip_quoted(&extract_body(payload))
}
