import { describe, expect, it } from "vitest";
import { canvasProgress, parseCanvasNodes } from "./canvas";

const canvas = `%%{ "seq": 2 }%%
flowchart TD
  002-N1["status: done<br/>summary: 스키마 확인"]
  002-N2["status: doing<br/>summary: 주입 경로 수정"]
  002-N3["status: todo<br/>summary: 테스트 추가"]
  002-N1 --&gt; 002-N2
  002-N2 --&gt; 002-N3
`;

describe("parseCanvasNodes", () => {
  it("노드를 선언 순서대로 뽑는다", () => {
    const nodes = parseCanvasNodes(canvas);
    expect(nodes.map((n) => n.id)).toEqual(["002-N1", "002-N2", "002-N3"]);
    expect(nodes.map((n) => n.status)).toEqual(["done", "doing", "todo"]);
    expect(nodes[1].summary).toBe("주입 경로 수정");
  });

  it("모르는 상태는 todo로 접는다", () => {
    const nodes = parseCanvasNodes('  001-N1["status: 저기요<br/>summary: x"]');
    expect(nodes[0].status).toBe("todo");
  });

  it("summary가 없는 노드는 버린다 — 그릴 것이 없다", () => {
    expect(parseCanvasNodes('  001-N1["status: done"]')).toEqual([]);
  });

  it("빈 캔버스와 형식이 어긋난 텍스트는 빈 배열", () => {
    expect(parseCanvasNodes("")).toEqual([]);
    expect(parseCanvasNodes("   \n")).toEqual([]);
    expect(parseCanvasNodes("flowchart TD\n  A --> B")).toEqual([]);
  });

  it("연속 호출에도 같은 결과 — 정규식 lastIndex가 새지 않는다", () => {
    expect(parseCanvasNodes(canvas)).toEqual(parseCanvasNodes(canvas));
  });

  it("seq가 999를 넘어 4자리가 돼도 노드를 놓치지 않는다", () => {
    const nodes = parseCanvasNodes('  1234-N1["status: done<br/>summary: 긴 세션"]');
    expect(nodes).toEqual([{ id: "1234-N1", status: "done", summary: "긴 세션" }]);
  });
});

describe("canvasProgress", () => {
  it("완료/전체를 센다", () => {
    expect(canvasProgress(parseCanvasNodes(canvas))).toEqual({ done: 1, total: 3 });
  });

  it("노드가 없으면 null", () => {
    expect(canvasProgress([])).toBeNull();
  });
});
