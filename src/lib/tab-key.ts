/**
 * 에디터 탭의 정체성 — `TabKey`.
 *
 * 같은 파일이 파일 탭과 diff 탭으로 **동시에** 열릴 수 있으므로 탭을 경로로 가리킬 수 없다.
 * 경로로 가리키면 diff 탭을 닫았는데 파일 탭이 닫히고, 트리에서 그 파일을 열면 "이미 열림"으로
 * 오인된다(ADR 0175 결정 7).
 *
 * **브랜드는 경로→키 방향만 잡는다.** `string & {brand}`는 `string` 슬롯에 그대로 대입되므로,
 * 키가 `Map<string, …>`·JSON 페이로드·`string[]` 인자로 새어 나가는 것은 타입이 잡지 못한다.
 * 그 방향의 지점은 설계 0061 DR-3이 손으로 열거하고 테스트가 잠근다.
 */

export type TabKey = string & { readonly __tabKey: unique symbol };

/** diff 탭 키의 접두. 경로에는 `:`가 들어갈 수 있지만 선두 `diff:`는 상대 경로에 나오지 않는다. */
const DIFF_PREFIX = "diff:";

/** 파일 탭의 키 = 경로 그대로. 파일 탭이 압도적 다수라 키와 경로가 같은 편이 읽기 쉽다. */
export function fileTabKey(path: string): TabKey {
  return path as TabKey;
}

/** diff 탭의 키. 같은 경로의 파일 탭과 공존해야 하므로 경로와 달라야 한다. */
export function diffTabKey(path: string): TabKey {
  return `${DIFF_PREFIX}${path}` as TabKey;
}

/** JSON·localStorage 왕복에서 브랜드가 소실된 문자열을 되돌린다. 그 지점에서만 쓴다. */
export function asTabKey(s: string): TabKey {
  return s as TabKey;
}

/** 이 탭이 diff인가 — 키만 들고 있는 자리(레이아웃·드래그)의 판정. */
export function isDiffKey(key: TabKey): boolean {
  return key.startsWith(DIFF_PREFIX);
}

/** 키가 가리키는 실경로. fs·LSP·Finder처럼 진짜 파일을 다루는 곳이 쓴다. */
export function pathFromTabKey(key: TabKey): string {
  return isDiffKey(key) ? key.slice(DIFF_PREFIX.length) : key;
}
