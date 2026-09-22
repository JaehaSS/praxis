// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PlanCalendar, type PlanCalendarApi } from "./PlanCalendar";
import type { DayItem } from "../today-items";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const item = (over: Partial<DayItem>): DayItem => ({
  id: 1,
  day: "2026-08-07",
  title: "제목",
  note: null,
  status: "open",
  position: 0,
  repo: null,
  task_id: null,
  source: "manual",
  source_ref: null,
  created_at: 0,
  updated_at: 0,
  done_at: null,
  carried_from: null,
  ...over,
});

/** 2026-08-07 12:00 KST — 격자와 "오늘"을 고정한다. */
const NOW = Date.parse("2026-08-07T03:00:00Z");

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = async (api: PlanCalendarApi, props: { onOpenHome?: () => void } = {}) => {
  await act(async () => {
    root.render(<PlanCalendar api={api} nowMs={NOW} {...props} />);
  });
};

const cellFor = (label: string) =>
  [...container.querySelectorAll("button")].find((b) =>
    b.getAttribute("aria-label")?.startsWith(label),
  );

describe("PlanCalendar", () => {
  it("표시 중인 달의 경계로 범위를 조회한다", async () => {
    const range = vi.fn().mockResolvedValue([]);
    await render({ range });

    expect(range).toHaveBeenCalledWith("2026-08-01", "2026-08-31");
  });

  it("오늘이 기본 선택이고 그날 항목만 보여준다", async () => {
    const range = vi.fn().mockResolvedValue([
      item({ id: 1, day: "2026-08-07", title: "오늘 것" }),
      item({ id: 2, day: "2026-08-03", title: "지난 것" }),
    ]);
    await render({ range });

    expect(container.textContent).toContain("8월 7일");
    expect(container.textContent).toContain("오늘 것");
    expect(container.textContent).not.toContain("지난 것");
  });

  it("날짜를 누르면 재조회 없이 목록이 바뀐다", async () => {
    const range = vi.fn().mockResolvedValue([
      item({ id: 1, day: "2026-08-07", title: "오늘 것" }),
      item({ id: 2, day: "2026-08-03", title: "지난 것" }),
    ]);
    await render({ range });

    await act(async () => {
      cellFor("8월 3일")?.click();
    });

    expect(container.textContent).toContain("지난 것");
    expect(container.textContent).not.toContain("오늘 것");
    // 월 범위를 이미 들고 있으므로 클릭은 IPC를 타지 않는다 (DR-3).
    expect(range).toHaveBeenCalledTimes(1);
  });

  it("달을 옮기면 그 달의 범위를 다시 조회한다", async () => {
    const range = vi.fn().mockResolvedValue([]);
    await render({ range });

    await act(async () => {
      container.querySelector<HTMLButtonElement>('[aria-label="다음 달"]')?.click();
    });

    expect(range).toHaveBeenLastCalledWith("2026-09-01", "2026-09-30");
    expect(container.textContent).toContain("2026년 9월");
  });

  it("다른 달로 옮기면 선택이 그 달 1일로 따라간다", async () => {
    const range = vi.fn().mockResolvedValue([]);
    await render({ range });

    await act(async () => {
      container.querySelector<HTMLButtonElement>('[aria-label="이전 달"]')?.click();
    });

    // 안 보이는 날이 선택된 채 남으면 목록이 이유 없이 비어 보인다.
    expect(container.textContent).toContain("7월 1일");
  });

  it("항목이 없는 날은 안내를 띄운다", async () => {
    await render({ range: vi.fn().mockResolvedValue([]) });

    expect(container.textContent).toContain("이 날은 계획한 일이 없습니다.");
  });

  it("dropped는 진행률 분모에서 빠진다", async () => {
    const range = vi.fn().mockResolvedValue([
      item({ id: 1, day: "2026-08-07", status: "done" }),
      item({ id: 2, day: "2026-08-07", status: "dropped" }),
    ]);
    await render({ range });

    expect(container.textContent).toContain("완료 1 / 1");
  });

  it("조회 실패를 섹션 안에서 알린다", async () => {
    await render({ range: vi.fn().mockRejectedValue(new Error("조회 실패")) });

    // 인사이트 상단 리포트와 독립적으로 자체 오류 상태를 갖는다.
    expect(container.textContent).toContain("조회 실패");
  });

  it("셀의 aria-label이 그날 진행 상황을 읽어준다", async () => {
    const range = vi.fn().mockResolvedValue([
      item({ id: 1, day: "2026-08-07", status: "done" }),
      item({ id: 2, day: "2026-08-07", status: "open" }),
    ]);
    await render({ range });

    expect(cellFor("8월 7일")?.getAttribute("aria-label")).toBe("8월 7일 · 완료 1/2");
  });

  it("Home 링크는 콜백이 있을 때만 나온다", async () => {
    const onOpenHome = vi.fn();
    await render({ range: vi.fn().mockResolvedValue([]) }, { onOpenHome });

    const link = [...container.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Home에서 편집"),
    );
    await act(async () => link?.click());
    expect(onOpenHome).toHaveBeenCalled();
  });

  it("콜백이 없으면 Home 링크를 숨긴다", async () => {
    await render({ range: vi.fn().mockResolvedValue([]) });

    expect(container.textContent).not.toContain("Home에서 편집");
  });
});
