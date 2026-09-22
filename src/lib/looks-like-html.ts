/** 확장자 없이 "이 텍스트 전체가 하나의 HTML 문서·블록인가"를 판정한다.
 *
 *  일부러 보수적이다. 채팅 메시지는 HTML을 **설명**하는 일이 훨씬 잦은데("`<table>` 태그는
 *  뭔가요?"), 그것을 문서로 오인해 iframe에 얹으면 대화가 통째로 사라진 것처럼 보인다.
 *  오탐의 대가가 미탐보다 크므로 "전체가 하나의 블록으로 닫혀 있을 때"만 참으로 본다 —
 *  앞뒤가 다른 텍스트로 둘러싸였거나 태그가 열린 채면 마크다운으로 넘긴다. */

/** 인라인 태그(`<span>`·`<b>`)는 제외한다 — 문서가 아니라 문장 속 장식이다. */
const BLOCK_TAGS = new Set([
  "table", "div", "section", "article", "main", "body", "ul", "ol", "dl",
  "figure", "figcaption", "p", "pre", "blockquote",
  "h1", "h2", "h3", "h4", "h5", "h6", "form",
]);

/** 앞뒤를 훑을 때 잘라 보는 길이. 선두 토큰도 끝 태그도 이 안에 들어온다. */
const EDGE = 256;

const nextNonSpace = (text: string, from: number): number => {
  const re = /\S/g;
  re.lastIndex = from;
  return re.exec(text)?.index ?? -1;
};

/** 생성기가 붙인 헤더 주석(`<!-- generated -->`)을 건너뛰고 첫 태그의 위치를 준다. */
function contentStart(text: string): number {
  let i = nextNonSpace(text, 0);
  while (i >= 0 && text.startsWith("<!--", i)) {
    const end = text.indexOf("-->", i + 4);
    if (end < 0) return -1;
    i = nextNonSpace(text, end + 3);
  }
  return i;
}

/** 선두 태그의 짝이 문서 **끝에서야** 닫히는가.
 *
 *  "같은 태그로 끝난다"만 보면 README의 흔한 형태 — `<div align="center">배지</div>` 로 시작해
 *  마크다운 본문을 지나 `<div>© 2026</div>` 로 끝나는 — 가 통과해 버린다. 그러면 제목·목록이
 *  iframe 안에서 글자 그대로 보인다. 깊이가 처음 0이 되는 지점이 마지막이어야 한 덩어리다. */
function closesOnlyAtEnd(text: string, tag: string, from: number): boolean {
  const re = new RegExp(`<(/?)${tag}(?=[\\s>/])`, "gi");
  re.lastIndex = from;
  let depth = 0;

  for (let m = re.exec(text); m; m = re.exec(text)) {
    depth += m[1] ? -1 : 1;
    if (depth > 0) continue;
    const close = text.indexOf(">", re.lastIndex);
    return close >= 0 && nextNonSpace(text, close + 1) < 0;
  }
  return false;
}

export function looksLikeHtml(text: string): boolean {
  // 싼 검사부터. `trim()`·`toLowerCase()`는 문자열 전체를 복사하는데 이 함수는 열려 있는 파일마다
  // 호출되므로, 앞뒤 EDGE 바이트만 훑고 O(n) 균형 순회는 그것들을 통과했을 때만 돈다.
  const start = contentStart(text);
  if (start < 0 || text[start] !== "<") return false;

  const tail = text.slice(-EDGE);
  if (!/>\s*$/.test(tail)) return false;

  // 문서 형태도 닫힘 검사를 받는다 — 스트리밍 중의 반쪽 문서가 통과하면 안 된다.
  const head = text.slice(start, start + EDGE);
  if (/^<!doctype\s+html/i.test(head) || /^<html[\s>]/i.test(head)) {
    return /<\/html\s*>\s*$/i.test(tail);
  }

  // 선두 태그명 — 속성이 붙든(`<table class="x">`) 안 붙든 이름까지만 본다.
  const tag = /^<([a-z][a-z0-9]*)(?=[\s>/])/i.exec(head)?.[1]?.toLowerCase();
  if (!tag || !BLOCK_TAGS.has(tag)) return false;
  if (!new RegExp(`</${tag}\\s*>\\s*$`, "i").test(tail)) return false;

  return closesOnlyAtEnd(text, tag, start);
}
