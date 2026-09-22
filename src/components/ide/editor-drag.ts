/**
 * 에디터로 끌어다 놓는 것들 — 운반 형식과 어느 가장자리인가의 판정.
 *
 * 끄는 쪽(탭 바·파일 트리)과 받는 쪽(분할 뷰)이 서로를 import 하지 않게 여기 한 벌만 둔다.
 * 둘이 각자 문자열을 적으면 한쪽만 고쳤을 때 드롭이 조용히 죽는다 — dataTransfer는 형식이
 * 안 맞아도 오류를 내지 않고 그냥 아무 일도 일어나지 않기 때문이다.
 */

import { asTabKey, type TabKey } from "../../lib/tab-key";
import type { DropEdge } from "./editor-split";

/** 편집 칸의 탭. 값은 `TabDragPayload`의 JSON. */
export const TAB_DRAG_MIME = "application/x-praxis-editor-tab";

/** 파일 트리의 파일 행. 값은 경로 문자열 하나. */
export const FILE_DRAG_MIME = "application/x-praxis-file-path";

export interface TabDragPayload {
  key: TabKey;
  /** 떠나온 칸. 드롭이 복제가 아니라 이동이 되려면 받는 쪽이 원본을 알아야 한다. */
  groupId: string;
}

/**
 * 지금 끌고 있는 것이 우리가 아는 것인가.
 *
 * `dragover` 단계에서는 dataTransfer가 보호 모드라 **값을 읽을 수 없고 타입만 보인다**.
 * 그래서 강조를 켜는 판단은 이 함수로, 실제 값은 `drop`에서 꺼낸다.
 */
export function draggedKind(types: readonly string[] | DOMStringList | undefined): "tab" | "file" | null {
  if (types == null) return null;
  const list = Array.from(types as ArrayLike<string>);
  if (list.includes(TAB_DRAG_MIME)) return "tab";
  if (list.includes(FILE_DRAG_MIME)) return "file";
  return null;
}

/** JSON을 거치면 브랜드가 소실되므로 `asTabKey`로 다시 씌운다 — 왕복의 유일한 복원 지점이다. */
export function readTabDrag(data: string): TabDragPayload | null {
  try {
    const parsed: unknown = JSON.parse(data);
    if (typeof parsed !== "object" || parsed == null) return null;
    const { key, groupId } = parsed as Partial<TabDragPayload>;
    return typeof key === "string" && typeof groupId === "string"
      ? { key: asTabKey(key), groupId }
      : null;
  } catch {
    return null;
  }
}

/**
 * 가장자리 판정 폭 — 칸의 짧은 쪽 대비 비율.
 *
 * 너무 좁으면 "가장자리에 갖다 놨는데 안 나뉜다"가 되고, 너무 넓으면 칸 안으로 옮기려던 것이
 * 자꾸 새 칸을 만든다. 1/4은 네 방향을 다 두고도 가운데가 절반은 남는 값이다.
 */
export const EDGE_RATIO = 0.25;

/**
 * 이 좌표가 칸의 어느 자리인가. 네 변까지의 거리를 비율로 재서 가장 가까운 하나를 고르고,
 * 그것마저 멀면 가운데다.
 *
 * 비율로 재는 이유는 칸이 가로로 길쭉해서다 — 픽셀로 재면 좁은 칸에서는 위아래 가장자리가
 * 칸을 통째로 덮고, 넓은 칸에서는 좌우 가장자리가 손톱만 해진다.
 */
export function edgeFromPoint(rect: DOMRect, x: number, y: number): DropEdge {
  if (rect.width <= 0 || rect.height <= 0) return "center";
  const rx = (x - rect.left) / rect.width;
  const ry = (y - rect.top) / rect.height;
  const candidates: Array<[DropEdge, number]> = [
    ["left", rx],
    ["right", 1 - rx],
    ["top", ry],
    ["bottom", 1 - ry],
  ];
  let best: [DropEdge, number] = candidates[0];
  for (const candidate of candidates) if (candidate[1] < best[1]) best = candidate;
  return best[1] < EDGE_RATIO ? best[0] : "center";
}

/**
 * 늘어선 것들 위 이 좌표가 몇 번째와 몇 번째 사이인가.
 *
 * 각 칸의 **가운데**를 경계로 삼는다. 앞 절반에 놓으면 그 앞, 뒤 절반이면 그 뒤 —
 * 경계를 칸 사이의 선으로 두면 폭 1px짜리 과녁을 맞혀야 한다.
 *
 * 가로로 늘어선 탭 바가 기본이고(`axis = "x"`), 세로로 쌓인 사이드바 그룹은 `"y"`로 부른다.
 * 세로판을 따로 복사하지 않는 이유는 규칙이 두 벌이 되면 한쪽만 고쳐지기 때문이다.
 *
 * 목록은 스크롤되므로 rect는 **화면 좌표**로 받는다(`getBoundingClientRect`).
 * 스크롤된 만큼을 따로 더하지 않아도 되는 것이 그 이유다.
 */
export function insertIndexFromPoint(
  rects: readonly DOMRect[],
  pos: number,
  axis: "x" | "y" = "x",
): number {
  for (let i = 0; i < rects.length; i++) {
    const start = axis === "x" ? rects[i].left : rects[i].top;
    const size = axis === "x" ? rects[i].width : rects[i].height;
    if (pos < start + size / 2) return i;
  }
  return rects.length;
}

/** 강조 사각형이 덮는 자리 — 가장자리는 절반, 가운데는 전부. */
export function dropZoneStyle(edge: DropEdge): {
  left: string;
  top: string;
  width: string;
  height: string;
} {
  switch (edge) {
    case "left":
      return { left: "0", top: "0", width: "50%", height: "100%" };
    case "right":
      return { left: "50%", top: "0", width: "50%", height: "100%" };
    case "top":
      return { left: "0", top: "0", width: "100%", height: "50%" };
    case "bottom":
      return { left: "0", top: "50%", width: "100%", height: "50%" };
    default:
      return { left: "0", top: "0", width: "100%", height: "100%" };
  }
}
