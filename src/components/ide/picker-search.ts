/**
 * 피커 검색의 순수 규칙 — 브랜치·레포·모델이 같은 한 벌을 쓴다(ADR 0142 확장, ADR 0176).
 * 좁히기만 하고 재정렬하지 않는다: 원래 순서가 곧 사용자가 기대하는 순서다.
 */

/** 질의를 공백으로 쪼갠 소문자 토큰. 빈 토큰은 버린다. */
export function queryTokens(query: string): string[] {
  return query.toLowerCase().trim().split(/\s+/).filter(Boolean);
}

/**
 * 토큰 AND 부분일치 — `jh2 tree`가 `feature/JH2-95-editor-popout-tree`를 찾는다.
 * fuzzy(subsequence)를 쓰지 않는 이유는 두세 글자 질의에 거의 모든 후보가 통과해
 * 좁히지 못하기 때문이다(설계 0010).
 *
 * `textOf`가 매칭 대상 문자열을 준다 — 레포는 전체 경로, 모델은 `label`+`id`다.
 */
export function filterByQuery<T>(items: T[], query: string, textOf: (item: T) => string): T[] {
  const tokens = queryTokens(query);
  if (tokens.length === 0) return items;
  return items.filter((item) => {
    const lower = textOf(item).toLowerCase();
    return tokens.every((token) => lower.includes(token));
  });
}

/** 토큰마다 첫 매치 구간만 모아 겹치는 것을 합친다 — 전부 칠하면 강조가 사라진다. */
export function matchRanges(text: string, tokens: string[]): [number, number][] {
  const lower = text.toLowerCase();
  const found: [number, number][] = [];
  for (const token of tokens) {
    const at = lower.indexOf(token);
    if (at >= 0) found.push([at, at + token.length]);
  }
  found.sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const range of found) {
    const last = merged[merged.length - 1];
    if (last && range[0] <= last[1]) last[1] = Math.max(last[1], range[1]);
    else merged.push([range[0], range[1]]);
  }
  return merged;
}
