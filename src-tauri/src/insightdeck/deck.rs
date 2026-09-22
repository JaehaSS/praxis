//! 인사이트 덱 파서 — `<app_data>/insight-decks/*.md` 하나가 한 덱이다.
//!
//! **플러그인이 아니라 데이터다** (플랜 0052 Architecture D). 필요한 것은 "코드를 고치지 않고
//! 내용을 추가"뿐이고 그건 파일 로더로 끝난다. 샌드박스·버전 협상·API 안정성은 아직 아무도
//! 요구하지 않았다.
//!
//! **TOML/YAML 파서를 쓰지 않는다.** 이 저장소는 `docs/subjects.yml`도 한 줄 고정 형식으로
//! 손수 읽는다 — 의존성 0을 지키기 위해서다. 형식은 원장(`docs/memory/`)의 관용을 그대로
//! 빌린다: 제목 한 줄 + 본문 + `- **field**: value`.
//!
//! ```markdown
//! # 보험 도메인
//!
//! ## 손해율과 합산비율은 다른 것을 잰다
//!
//! 발생손해액 ÷ 경과보험료가 손해율이다. 여기에 사업비율을 더한 것이 합산비율이고,
//! 언더라이팅 손익이 흑자인지는 후자로 판단한다.
//!
//! - **source**: 보험업감독업무시행세칙 별표
//! - **tags**: 손해보험, 지표
//! ```
//!
//! Tauri 비의존 — `cargo test`로 직접 검증된다.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InsightCard {
    /// 덱 안에서 이 카드를 가리키는 키 — `<덱파일스템>#<제목>`. 재출제 억제의 식별자다.
    pub key: String,
    pub deck: String,
    pub title: String,
    pub body: String,
    /// **필수.** 대기 시간에 스치듯 읽는 화면일수록 출처 없는 단정이 그대로 믿음이 된다.
    /// 퀴즈가 `source_excerpt`를 요구하는 것과 같은 이유다(설계 0044).
    pub source: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Deck {
    pub name: String,
    pub cards: Vec<InsightCard>,
}

/// 한 줄 필드(`- **name**: value`)를 읽는다. 없으면 `None`.
fn field(lines: &[&str], name: &str) -> Option<String> {
    let prefix = format!("- **{name}**:");
    lines
        .iter()
        .find_map(|l| l.trim().strip_prefix(&prefix))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// 덱 하나를 읽는다.
///
/// **부분 실패가 전체를 막지 않는다** — 카드 하나가 `source` 없이 들어와도 그 카드만
/// 버리고 나머지는 살린다. 반환의 두 번째 값이 버린 이유들이다.
pub fn parse_deck(stem: &str, text: &str) -> (Option<Deck>, Vec<String>) {
    let text = text.replace("\r\n", "\n");
    let mut warnings = Vec::new();

    let name = text
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(name) = name else {
        warnings.push(format!("{stem}: 맨 위 '# 덱 이름' 한 줄이 없습니다"));
        return (None, warnings);
    };

    let mut cards = Vec::new();
    // `## 제목`으로 카드를 가른다. 첫 조각(덱 머리말)은 카드가 아니다.
    for chunk in text.split("\n## ").skip(1) {
        let mut lines = chunk.lines();
        let Some(title) = lines.next().map(str::trim).filter(|t| !t.is_empty()) else {
            continue;
        };
        let rest: Vec<&str> = lines.collect();

        let Some(source) = field(&rest, "source") else {
            warnings.push(format!("{stem} / {title}: source가 없어 버렸습니다"));
            continue;
        };
        // 본문 = 필드 줄과 빈 줄을 뺀 나머지. 여러 문단이면 공백으로 잇는다.
        let body = rest
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with("- **"))
            .collect::<Vec<_>>()
            .join(" ");
        if body.is_empty() {
            warnings.push(format!("{stem} / {title}: 본문이 비어 버렸습니다"));
            continue;
        }
        let tags = field(&rest, "tags")
            .map(|v| {
                v.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        cards.push(InsightCard {
            key: format!("{stem}#{title}"),
            deck: name.clone(),
            title: title.to_string(),
            body,
            source,
            tags,
        });
    }

    if cards.is_empty() {
        warnings.push(format!("{stem}: 쓸 수 있는 카드가 없습니다"));
        return (None, warnings);
    }
    (Some(Deck { name, cards }), warnings)
}

/// 디렉터리 전체를 읽는다.
///
/// **없으면 빈 목록이다 — 에러가 아니다.** 아무도 덱을 안 만든 상태가 기본이고,
/// 거기서 에러를 내면 앱이 매번 경고한다.
pub fn load_decks(dir: &Path) -> (Vec<Deck>, Vec<String>) {
    let mut decks = Vec::new();
    let mut warnings = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (decks, warnings);
    };
    // 파일명 순 — 어떤 순서로 읽히든 같은 결과가 나오게.
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    paths.sort();

    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("deck")
            .to_string();
        let Ok(text) = std::fs::read_to_string(&path) else {
            warnings.push(format!("{stem}: 읽지 못했습니다"));
            continue;
        };
        // 한 파일이 깨져도 나머지는 살린다.
        let (deck, mut w) = parse_deck(&stem, &text);
        warnings.append(&mut w);
        if let Some(d) = deck {
            decks.push(d);
        }
    }
    (decks, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "# 보험 도메인\n\n\
        ## 손해율과 합산비율은 다른 것을 잰다\n\n\
        발생손해액 ÷ 경과보험료가 손해율이다.\n\n\
        - **source**: 보험업감독업무시행세칙 별표\n\
        - **tags**: 손해보험, 지표\n\n\
        ## 위험률차익은 예정과 실제의 차다\n\n\
        예정위험률보다 실제 사고가 적으면 남는다.\n\n\
        - **source**: 보험계리 표준\n";

    #[test]
    fn parses_a_deck_with_cards() {
        let (deck, w) = parse_deck("insurance", GOOD);
        let deck = deck.expect("덱이 없다");
        assert_eq!(deck.name, "보험 도메인");
        assert_eq!(deck.cards.len(), 2);
        assert_eq!(deck.cards[0].tags, vec!["손해보험", "지표"]);
        assert_eq!(deck.cards[1].tags, Vec::<String>::new());
        assert!(w.is_empty(), "경고가 있다: {w:?}");
    }

    #[test]
    fn a_card_without_source_is_rejected_but_the_deck_survives() {
        // 이 규칙이 BR-6의 전부다 — 출처 없는 단정을 대기 화면에 띄우지 않는다.
        let text = format!("{GOOD}\n## 출처 없는 주장\n\n근거 없이 단정한다.\n");
        let (deck, w) = parse_deck("insurance", &text);
        let deck = deck.expect("덱이 통째로 죽었다");
        assert_eq!(deck.cards.len(), 2, "출처 없는 카드가 살아남았다");
        assert!(w.iter().any(|m| m.contains("source가 없어")), "{w:?}");
    }

    #[test]
    fn an_empty_body_is_rejected() {
        let text = "# D\n\n## 제목뿐\n\n- **source**: x\n";
        let (deck, w) = parse_deck("d", text);
        assert!(deck.is_none());
        assert!(w.iter().any(|m| m.contains("본문이 비어")), "{w:?}");
    }

    #[test]
    fn a_deck_without_a_title_is_rejected() {
        let (deck, w) = parse_deck("d", "## 카드\n\n본문\n\n- **source**: x\n");
        assert!(deck.is_none());
        assert!(w.iter().any(|m| m.contains("덱 이름")), "{w:?}");
    }

    #[test]
    fn a_missing_directory_yields_no_decks_and_no_error() {
        // 아무도 덱을 안 만든 상태가 기본이다. 여기서 에러를 내면 앱이 매번 경고한다.
        let (decks, w) = load_decks(Path::new("/nonexistent/praxis-decks"));
        assert!(decks.is_empty());
        assert!(w.is_empty());
    }

    #[test]
    fn a_malformed_file_does_not_kill_the_others() {
        let dir = std::env::temp_dir().join(format!("praxis-decks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a-broken.md"), "제목이 없다").unwrap();
        std::fs::write(dir.join("b-good.md"), GOOD).unwrap();

        let (decks, w) = load_decks(&dir);
        assert_eq!(decks.len(), 1, "깨진 파일이 나머지를 죽였다");
        assert_eq!(decks[0].cards.len(), 2);
        assert!(!w.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 우리가 배포하는 예시 덱이 **실제 파서를 통과하는지** 본다.
    ///
    /// 형식의 본보기로 내놓은 파일이 정작 안 읽히면 사용자는 자기 덱이 틀린 줄 안다.
    /// 형식을 바꾸면 이 테스트가 먼저 깨져야 한다.
    #[test]
    fn the_shipped_example_deck_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/examples/insight-deck-insurance.md");
        let text = std::fs::read_to_string(&path).expect("예시 덱이 없다");
        let (deck, w) = parse_deck("insight-deck-insurance", &text);
        let deck = deck.expect("예시 덱이 파싱되지 않는다");
        assert!(deck.cards.len() >= 2, "예시 카드가 {}장뿐", deck.cards.len());
        assert!(w.is_empty(), "예시 덱에 버려진 카드가 있다: {w:?}");
        for c in &deck.cards {
            assert!(!c.source.trim().is_empty(), "{}: 출처가 비었다", c.title);
            assert!(!c.body.trim().is_empty(), "{}: 본문이 비었다", c.title);
        }
    }

    #[test]
    fn keys_are_unique_across_decks() {
        // 같은 제목이 두 덱에 있어도 재출제 억제가 서로를 지우면 안 된다.
        let (a, _) = parse_deck("insurance", GOOD);
        let (b, _) = parse_deck("finance", GOOD);
        let ka: Vec<_> = a.unwrap().cards.iter().map(|c| c.key.clone()).collect();
        let kb: Vec<_> = b.unwrap().cards.iter().map(|c| c.key.clone()).collect();
        assert!(ka.iter().all(|k| !kb.contains(k)), "키가 겹친다");
    }
}
