import { describe, expect, it } from "vitest";
import {
  draggedKind,
  dropZoneStyle,
  edgeFromPoint,
  readTabDrag,
  EDGE_RATIO,
  FILE_DRAG_MIME,
  TAB_DRAG_MIME,
  insertIndexFromPoint,
} from "./editor-drag";
import { diffTabKey, isDiffKey } from "../../lib/tab-key";

const rect = (width: number, height: number): DOMRect =>
  ({ left: 0, top: 0, width, height, right: width, bottom: height, x: 0, y: 0 }) as DOMRect;

describe("edgeFromPoint", () => {
  const wide = rect(1000, 400);

  it("가운데는 가운데다", () => {
    expect(edgeFromPoint(wide, 500, 200)).toBe("center");
  });

  it("네 변 각각을 알아본다", () => {
    expect(edgeFromPoint(wide, 20, 200)).toBe("left");
    expect(edgeFromPoint(wide, 980, 200)).toBe("right");
    expect(edgeFromPoint(wide, 500, 10)).toBe("top");
    expect(edgeFromPoint(wide, 500, 390)).toBe("bottom");
  });

  it("모서리에서는 더 가까운 변이 이긴다", () => {
    // 왼쪽에서 2%, 위쪽에서 5% — 왼쪽이 더 가깝다.
    expect(edgeFromPoint(wide, 20, 20)).toBe("left");
    expect(edgeFromPoint(wide, 100, 4)).toBe("top");
  });

  it("판정 폭은 칸의 비율로 잰다 — 길쭉해도 네 변이 다 남는다", () => {
    const tall = rect(300, 1200);
    const inside = tall.height * EDGE_RATIO * 0.5;
    expect(edgeFromPoint(tall, 150, inside)).toBe("top");
    // 같은 픽셀 거리라도 가로로는 가운데다 — 폭 300의 절반 지점.
    expect(edgeFromPoint(tall, 150, 600)).toBe("center");
  });

  it("크기가 0인 칸은 가운데로 떨어진다 — 나눌 자리가 없다", () => {
    expect(edgeFromPoint(rect(0, 0), 0, 0)).toBe("center");
  });
});

describe("draggedKind", () => {
  it("아는 형식만 알아본다", () => {
    expect(draggedKind([TAB_DRAG_MIME])).toBe("tab");
    expect(draggedKind([FILE_DRAG_MIME, "text/plain"])).toBe("file");
    expect(draggedKind(["text/plain"])).toBeNull();
    expect(draggedKind(undefined)).toBeNull();
  });
});

describe("readTabDrag", () => {
  it("온전한 payload만 통과시킨다", () => {
    expect(readTabDrag(JSON.stringify({ key: "a.ts", groupId: "g0" }))).toEqual({
      key: "a.ts",
      groupId: "g0",
    });
    expect(readTabDrag('{"key":"a.ts"}')).toBeNull();
    expect(readTabDrag("망가진 json")).toBeNull();
    expect(readTabDrag("null")).toBeNull();
  });

  // AC-14 — JSON 왕복이 브랜드를 지우는 유일한 지점이다. 접두를 잃으면 옮겨 놓은 diff 탭이
  // 같은 이름의 파일 탭으로 되살아난다.
  it("diff 키의 접두를 왕복에서 잃지 않는다 (AC-14)", () => {
    const key = diffTabKey("src/a.ts");
    const restored = readTabDrag(JSON.stringify({ key, groupId: "g0" }));

    expect(restored?.key).toBe(key);
    expect(isDiffKey(restored!.key)).toBe(true);
  });
});

describe("dropZoneStyle", () => {
  it("가장자리는 절반, 가운데는 전부를 덮는다", () => {
    expect(dropZoneStyle("right")).toMatchObject({ left: "50%", width: "50%", height: "100%" });
    expect(dropZoneStyle("bottom")).toMatchObject({ top: "50%", height: "50%", width: "100%" });
    expect(dropZoneStyle("center")).toMatchObject({ width: "100%", height: "100%" });
  });
});

describe("탭 바 삽입 자리", () => {
  /** 폭 100짜리 탭 셋이 왼쪽부터 늘어선 탭 바. */
  const rects = [0, 100, 200].map(
    (left) => ({ left, width: 100 }) as DOMRect,
  );

  it("탭의 왼쪽 절반이면 그 앞이다", () => {
    expect(insertIndexFromPoint(rects, 10)).toBe(0);
    expect(insertIndexFromPoint(rects, 120)).toBe(1);
  });

  it("탭의 오른쪽 절반이면 그 뒤다", () => {
    expect(insertIndexFromPoint(rects, 60)).toBe(1);
    expect(insertIndexFromPoint(rects, 160)).toBe(2);
  });

  it("마지막 탭을 지나면 끝이다", () => {
    expect(insertIndexFromPoint(rects, 400)).toBe(3);
  });

  it("탭이 없으면 언제나 0", () => {
    expect(insertIndexFromPoint([], 999)).toBe(0);
  });

  // 사이드바 그룹은 세로로 쌓인다 — 세로판을 따로 만들면 규칙이 두 벌이 된다.
  it("세로 축에서는 박스 가운데가 경계다", () => {
    const boxes = [0, 100, 200].map((top) => ({ top, height: 100 }) as DOMRect);

    expect(insertIndexFromPoint(boxes, 10, "y")).toBe(0);
    expect(insertIndexFromPoint(boxes, 160, "y")).toBe(2);
    expect(insertIndexFromPoint(boxes, 400, "y")).toBe(3);
  });
});
