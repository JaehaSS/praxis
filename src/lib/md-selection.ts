/**
 * 마크다운 **프리뷰에서 고른 텍스트**가 원본 파일의 몇 번째 줄인가.
 *
 * 프리뷰는 렌더된 DOM이라 원본과 글자가 다르다(`**굵게**` → `굵게`). 그래서 선택 텍스트를
 * 원본에서 되찾는 방식은 쓸 수 없다 — 같은 문장이 두 번 나오면 어느 쪽인지 정할 수 없고,
 * 강조·링크가 섞이면 애초에 일치하지 않는다.
 *
 * 대신 렌더할 때 블록마다 원본 줄 번호를 심어 둔다(`MarkdownDoc`). 여기서 하는 일은 선택
 * 지점에서 위로 올라가며 그 표식을 찾는 것뿐이다. 블록 단위라 문단 중간을 골라도 문단의
 * 시작·끝 줄로 넓혀지는데, 첨부의 목적이 "어디를 말하는지"를 알리는 것이므로 그 정도면 된다.
 */

export const MD_LINE_ATTR = "data-md-line";
export const MD_LINE_END_ATTR = "data-md-line-end";

/** 이 노드가 속한 블록의 원본 줄. 표식을 단 조상이 없으면 null. */
export function lineOfNode(node: Node | null, edge: "start" | "end"): number | null {
  const attr = edge === "start" ? MD_LINE_ATTR : MD_LINE_END_ATTR;
  let el: Element | null =
    node == null ? null : node.nodeType === 1 ? (node as Element) : node.parentElement;
  while (el != null) {
    const raw = el.getAttribute(attr);
    // 가장 가까운 블록이 이긴다 — 표 안의 셀은 표 전체가 아니라 그 셀의 줄을 뜻한다.
    if (raw != null) {
      const line = Number(raw);
      if (Number.isFinite(line) && line > 0) return line;
    }
    el = el.parentElement;
  }
  return null;
}

/**
 * 선택 범위를 원본 줄 범위로. 양 끝 중 한쪽이라도 블록을 못 찾으면 다른 쪽으로 메운다 —
 * 문서 여백까지 걸친 선택이 통째로 버려지면 "드래그했는데 아무 일도 안 난다"가 된다.
 */
export function linesOfRange(range: Range): { startLine: number; endLine: number } | null {
  const start = lineOfNode(range.startContainer, "start");
  const end = lineOfNode(range.endContainer, "end");
  const from = start ?? end;
  const to = end ?? start;
  if (from == null || to == null) return null;
  // 뒤에서 앞으로 끈 선택은 Range가 이미 바로잡아 주지만, 블록 표식은 끝 블록이 시작 블록보다
  // 앞일 수 있다(중첩 목록). 순서는 여기서 한 번 더 확인한다.
  return from <= to ? { startLine: from, endLine: to } : { startLine: to, endLine: from };
}
