import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { CanvasSection } from "./CapsulePanel";

const canvas = `%%{ "seq": 1 }%%
flowchart TD
  001-N1["status: done<br/>summary: 스키마 확인"]
  001-N2["status: doing<br/>summary: 주입 경로 수정"]
  001-N3["status: todo<br/>summary: 테스트 추가"]
`;

describe("CanvasSection", () => {
  it("노드를 순서대로 보여주고 진행 수를 센다", () => {
    const html = renderToStaticMarkup(<CanvasSection canvas={canvas} />);
    expect(html).toContain("작업 캔버스 · 1/3");
    expect(html).toContain("스키마 확인");
    expect(html).toContain("주입 경로 수정");
    expect(html).toContain("테스트 추가");
  });

  it("상태를 색과 기호로 구분한다", () => {
    const html = renderToStaticMarkup(<CanvasSection canvas={canvas} />);
    expect(html).toContain("text-status-done");
    expect(html).toContain("text-status-running");
  });

  it("계획이 없으면 섹션 자체를 그리지 않는다", () => {
    expect(renderToStaticMarkup(<CanvasSection canvas="" />)).toBe("");
    expect(renderToStaticMarkup(<CanvasSection canvas="flowchart TD" />)).toBe("");
  });
});
