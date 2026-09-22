import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { InsightsView } from "./InsightsView";

describe("InsightsView", () => {
  it("섹션을 질문 축 다섯으로 싣는다", () => {
    const html = renderToStaticMarkup(<InsightsView />);

    // 탭이 아니라 단일 스크롤 리포트의 섹션이다(ADR 0033) — 점프 칩과 섹션이 함께 있어야 한다.
    for (const label of ["요약", "지출", "작업", "방식", "회고"]) {
      expect(html).toContain(`>${label}<`);
    }
    expect(html).toContain('id="summary"');
    expect(html).toContain('id="tasks"');
    expect(html).toContain('id="retro"');
  });

  it("사용량 집계가 비어 있어도 작업·회고 섹션은 렌더된다", () => {
    // 출처가 다르다(트랜스크립트 vs 작업 DB) — 한쪽 공백이 다른 쪽을 가리면 안 된다.
    const html = renderToStaticMarkup(<InsightsView />);

    expect(html).toContain("집계 중…");
    expect(html).toContain('id="tasks"');
    expect(html).toContain('id="retro"');
  });

  it("AX 결과를 지우지 않고 접어둔다", () => {
    // goal_contract를 쓴 작업이 사실상 없어 지표가 0에 수렴하지만, 지표가 0인 것과
    // 기능이 필요 없는 것은 다르다(설계 0054 DR-8).
    const html = renderToStaticMarkup(<InsightsView />);

    expect(html).toContain("<details");
    expect(html).toContain("AX 결과");
  });
});
