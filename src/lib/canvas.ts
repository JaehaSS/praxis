/**
 * 작업 캔버스(Mermaid 텍스트) → 화면용 노드 목록.
 *
 * Mermaid 런타임을 번들에 넣지 않는다(계획 0033 DR-5) — 캔버스의 가치는 에이전트가 읽는
 * 기호 밀도에 있지 사람이 보는 그림에 있지 않고, 사람에게는 목록이 더 읽기 쉽다.
 *
 * 백엔드가 구조화 배열을 따로 내려주지 않고 **주입되는 텍스트 그대로** 파싱하는 이유는,
 * 화면에 보이는 것과 다음 세션이 받는 것이 어긋날 여지를 없애기 위해서다.
 */

export type CanvasStatus = "done" | "doing" | "todo";

export interface CanvasNode {
  id: string;
  status: CanvasStatus;
  summary: string;
}

// seq는 0 패딩 3자리가 기본이지만 999를 넘으면 넓어진다 — 고정폭으로 잡으면 긴 세션에서
// 노드를 통째로 놓친다.
const NODE = /(\d{3,}-N\d+)\["([^"]*)"\]/g;

const asStatus = (value: string): CanvasStatus =>
  value === "done" || value === "doing" ? value : "todo";

/** Mermaid 캔버스에서 노드를 순서대로 뽑는다. 형식이 어긋나면 빈 배열 — 추측하지 않는다. */
export function parseCanvasNodes(canvas: string): CanvasNode[] {
  if (!canvas.trim()) return [];
  const nodes: CanvasNode[] = [];
  for (const match of canvas.matchAll(NODE)) {
    const [, id, label] = match;
    const status = label.match(/status:\s*(\w+)/);
    const summary = label.match(/summary:\s*(.*?)(?:<br\/>|$)/);
    if (!summary) continue;
    nodes.push({
      id,
      status: asStatus(status?.[1] ?? ""),
      summary: summary[1].trim(),
    });
  }
  return nodes;
}

/** 진행 요약 — 완료/전체. 노드가 없으면 null. */
export function canvasProgress(nodes: CanvasNode[]): { done: number; total: number } | null {
  if (nodes.length === 0) return null;
  return { done: nodes.filter((n) => n.status === "done").length, total: nodes.length };
}
