//! 다음에 낼 카드를 고른다 — 최근에 보인 것은 뒤로 민다.

use super::deck::{Deck, InsightCard};

/// 최근 이만큼은 다시 내지 않는다.
///
/// 덱이 이보다 작으면 상한이 **덱 크기 - 1**로 줄어든다 — 아니면 카드가 3장인 덱에서
/// 영영 아무것도 못 낸다.
pub const RECENT_WINDOW: usize = 10;

/// 모든 덱의 카드를 한 줄로 편다.
pub fn all_cards(decks: &[Deck]) -> Vec<&InsightCard> {
    decks.iter().flat_map(|d| d.cards.iter()).collect()
}

/// 다음에 낼 카드.
///
/// `recent`는 최근에 보인 키를 **새것부터** 나열한 것이다. `seed`는 호출부가 넘기는
/// 결정성 있는 값 — 같은 상태에 같은 seed면 같은 카드가 나온다(테스트 가능).
///
/// 우선순위는 둘이다. ① 최근 창에 없는 카드 ② 그중 **최근 카드와 태그가 겹치지 않는** 것.
/// 같은 주제가 연달아 나오면 "새로운 것"이라는 느낌이 사라진다.
pub fn pick_next<'a>(
    decks: &'a [Deck],
    recent: &[String],
    seed: u64,
) -> Option<&'a InsightCard> {
    let cards = all_cards(decks);
    if cards.is_empty() {
        return None;
    }
    // 덱이 창보다 작으면 창을 줄인다 — 그러지 않으면 후보가 0이 되어 영영 못 낸다.
    let window = RECENT_WINDOW.min(cards.len().saturating_sub(1));
    let blocked: Vec<&String> = recent.iter().take(window).collect();

    let fresh: Vec<&&InsightCard> = cards
        .iter()
        .filter(|c| !blocked.iter().any(|k| **k == c.key))
        .collect();
    // 전부 최근에 나왔다면(창을 줄여도) 가장 오래된 것부터 다시 낸다.
    let pool: Vec<&&InsightCard> = if fresh.is_empty() {
        cards.iter().collect()
    } else {
        fresh
    };

    let last_tags: Vec<&String> = recent
        .first()
        .and_then(|k| cards.iter().find(|c| &c.key == k))
        .map(|c| c.tags.iter().collect())
        .unwrap_or_default();

    // 태그가 겹치지 않는 것을 먼저 본다. 그런 게 없으면 전체에서 고른다.
    let preferred: Vec<&&InsightCard> = pool
        .iter()
        .copied()
        .filter(|c| !c.tags.iter().any(|t| last_tags.contains(&t)))
        .collect();
    let final_pool = if preferred.is_empty() { pool } else { preferred };

    let idx = (seed % final_pool.len() as u64) as usize;
    Some(final_pool[idx])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insightdeck::deck::parse_deck;

    fn deck(n: usize) -> Vec<Deck> {
        let mut text = String::from("# T\n");
        for i in 0..n {
            text.push_str(&format!(
                "\n## 카드{i}\n\n본문{i}\n\n- **source**: s{i}\n"
            ));
        }
        let (d, _) = parse_deck("t", &text);
        vec![d.unwrap()]
    }

    #[test]
    fn does_not_repeat_the_last_card() {
        let d = deck(5);
        let last = d[0].cards[0].key.clone();
        for seed in 0..20 {
            let got = pick_next(&d, std::slice::from_ref(&last), seed).unwrap();
            assert_ne!(got.key, last, "직전 카드를 다시 냈다 (seed {seed})");
        }
    }

    #[test]
    fn a_deck_smaller_than_the_window_still_serves() {
        // 카드 2장 + 창 10 → 후보가 0이 되어 무한 None이 되면 안 된다.
        let d = deck(2);
        let keys: Vec<String> = d[0].cards.iter().map(|c| c.key.clone()).collect();
        let got = pick_next(&d, &keys, 0).expect("작은 덱에서 아무것도 못 냈다");
        assert!(keys.contains(&got.key));
    }

    #[test]
    fn a_single_card_deck_keeps_serving_it() {
        let d = deck(1);
        let k = d[0].cards[0].key.clone();
        assert_eq!(pick_next(&d, std::slice::from_ref(&k), 0).unwrap().key, k);
    }

    #[test]
    fn no_decks_yields_none() {
        assert!(pick_next(&[], &[], 0).is_none());
    }

    #[test]
    fn is_deterministic_for_the_same_seed() {
        let d = deck(7);
        let a = pick_next(&d, &[], 42).unwrap().key.clone();
        let b = pick_next(&d, &[], 42).unwrap().key.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn prefers_a_different_tag_than_the_last_card() {
        let text = "# T\n\n## A\n\n본문\n\n- **source**: s\n- **tags**: 손해보험\n\n\
                    ## B\n\n본문\n\n- **source**: s\n- **tags**: 손해보험\n\n\
                    ## C\n\n본문\n\n- **source**: s\n- **tags**: 생명보험\n";
        let (d, _) = parse_deck("t", text);
        let d = vec![d.unwrap()];
        let a_key = d[0].cards[0].key.clone();
        // 직전이 A(손해보험)면 같은 태그의 B보다 C가 나와야 한다.
        for seed in 0..10 {
            let got = pick_next(&d, std::slice::from_ref(&a_key), seed).unwrap();
            assert_eq!(got.title, "C", "태그가 겹치는 카드를 골랐다 (seed {seed})");
        }
    }
}
