import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { TaskPatternPanel } from "./TaskPatternPanel";
import type { TaskPatterns } from "../../../lib/ipc";

const base: TaskPatterns = {
  funnel: { total: 100, started: 95, reviewed: 80, done: 50, discarded: 30, failed: 5 },
  states: [
    { state: "Done", count: 50 },
    { state: "Discarded", count: 30 },
  ],
  discard_trend: [
    { month: "2026-06", total: 7, discarded: 1 },
    { month: "2026-08", total: 332, discarded: 117 },
  ],
  followup: { total: 100, with_followup: 58 },
  role_outcomes: [{ role: "implementer", count: 80, done: 38, discarded: 25 }],
  duration_p50: 720,
  duration_p90: 2820,
};

describe("TaskPatternPanel", () => {
  it("집계가 오기 전에는 자리를 지킨다", () => {
    expect(renderToStaticMarkup(<TaskPatternPanel data={null} />)).toContain("집계 중…");
  });

  it("작업이 없으면 빈 상태를 말한다", () => {
    const empty: TaskPatterns = {
      ...base,
      funnel: { total: 0, started: 0, reviewed: 0, done: 0, discarded: 0, failed: 0 },
    };
    expect(renderToStaticMarkup(<TaskPatternPanel data={empty} />)).toContain(
      "이 기간에 작업 기록이 없습니다",
    );
  });

  it("퍼널의 이탈을 함께 보여준다", () => {
    const html = renderToStaticMarkup(<TaskPatternPanel data={base} />);

    // 도달 수만 보여주면 "어디서 새는가"가 안 보인다.
    expect(html).toContain("큐에서 멈춤");
    expect(html).toContain("30 폐기");
    expect(html).toContain("5 실패");
  });

  it("폐기 추세를 월별로 모두 싣는다", () => {
    // 범위 칩으로 잘리지 않는 레인이다 — 두 달이 다 있어야 상승이 보인다(설계 0054 §6.2).
    const html = renderToStaticMarkup(<TaskPatternPanel data={base} />);

    expect(html).toContain("2026-06");
    expect(html).toContain("2026-08");
    expect(html).toContain("35%");
    expect(html).toContain("범위 칩과 무관");
  });

  it("소요 표본이 없으면 0이 아니라 —로 쓴다", () => {
    const noSample: TaskPatterns = { ...base, duration_p50: null, duration_p90: null };
    const html = renderToStaticMarkup(<TaskPatternPanel data={noSample} />);

    expect(html).toContain("—");
    expect(html).not.toContain("0초");
  });

  it("후속 입력을 비율로 쓴다", () => {
    const html = renderToStaticMarkup(<TaskPatternPanel data={base} />);
    expect(html).toContain("58%");
  });
});
