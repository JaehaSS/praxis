// src/lib/mention.ts — @파일 멘션 순수 로직 (UI 무관, 컴포저 공유).

/** 캐럿 앞 문자열에서 활성 @토큰 추출. @가 줄머리이거나 공백 뒤일 때만. 없으면 null. */
export function matchMentionToken(before: string): string | null {
  const m = before.match(/(?:^|\s)@([^\s@]*)$/);
  return m ? m[1] : null;
}

/** 파일 경로 목록을 토큰(대소문자 무시 substring)으로 필터. 최대 limit개. */
export function filterMentionFiles(files: string[], token: string, limit = 8): string[] {
  const lc = token.toLowerCase();
  return files.filter((f) => f.toLowerCase().includes(lc)).slice(0, limit);
}

/** `/` 프리픽스로 스킬을 거른다. 이름 앞부분 일치가 먼저 오고, 플러그인 스킬
 *  (`<plugin>:<name>`)은 뒷마디로도 닿게 해 그 뒤에 붙인다 — 접두어를 외우지 않아도
 *  `/rescue`로 `codex:rescue`를 찾을 수 있다. 최대 limit개. */
export function filterSkills<T extends { name: string }>(
  list: T[],
  prefix: string,
  limit = 25,
): T[] {
  const lc = prefix.toLowerCase();
  if (!lc) return list.slice(0, limit);
  const head: T[] = [];
  const tail: T[] = [];
  for (const item of list) {
    const name = item.name.toLowerCase();
    if (name.startsWith(lc)) head.push(item);
    else if (name.includes(":") && name.slice(name.indexOf(":") + 1).startsWith(lc))
      tail.push(item);
  }
  return head.concat(tail).slice(0, limit);
}

/** value의 caret 위치 @토큰을 `@path `로 치환. 새 value와 캐럿 위치 반환.
 *  replacer는 함수로 넘긴다 — path에 `$1`·`$&` 등이 있어도 치환 패턴이 아닌 리터럴로 삽입. */
export function applyMention(
  value: string,
  caret: number,
  path: string,
): { value: string; caret: number } {
  const before = value.slice(0, caret).replace(/@([^\s@]*)$/, () => `@${path} `);
  return { value: before + value.slice(caret), caret: before.length };
}

/** 컴포저 자동완성 드롭다운(@파일·/스킬 공유) 열림 시 ↑↓/Tab/Enter/Esc 키를 처리.
 *  처리했으면 true(호출부는 return). ArrowDown/Up은 sel 이동, Tab은 현재 항목 선택,
 *  Esc는 닫기. Enter는 `enterSelects`일 때만 선택(그 외엔 fallthrough — 전송/생성으로).
 *  그 외 키는 false. 항목 타입 T에 무관(경로 문자열·SkillMeta 등 공용). */
export function handleMenuKey<T>(
  e: { key: string; preventDefault: () => void },
  opts: {
    items: T[];
    sel: number;
    setSel: (updater: (s: number) => number) => void;
    onSelect: (item: T) => void;
    close: () => void;
    /** Enter로도 선택할지 (기본 false — Tab만). @파일 멘션은 true. */
    enterSelects?: boolean;
  },
): boolean {
  const { items, sel, setSel, onSelect, close, enterSelects } = opts;
  if (e.key === "ArrowDown") {
    e.preventDefault();
    setSel((s) => Math.min(s + 1, items.length - 1));
    return true;
  }
  if (e.key === "ArrowUp") {
    e.preventDefault();
    setSel((s) => Math.max(s - 1, 0));
    return true;
  }
  if (e.key === "Tab" || (enterSelects && e.key === "Enter")) {
    e.preventDefault();
    onSelect(items[sel]);
    return true;
  }
  if (e.key === "Escape") {
    e.preventDefault();
    close();
    return true;
  }
  return false;
}
