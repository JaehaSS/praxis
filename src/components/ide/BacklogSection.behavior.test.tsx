// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BacklogSection, type BacklogApi } from "./BacklogSection";
import type { DayItem } from "./today-items";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** 테스트의 "지금" — 2026-08-27T00:00:00Z. 적체 일수를 손으로 셀 수 있게 고정한다. */
const NOW_MS = 1_787_875_200_000;
const NOW_SECS = NOW_MS / 1000;
const DAY = 86_400;

const item = (over: Partial<DayItem>): DayItem => ({
  id: 1,
  day: "backlog",
  title: "언젠가 할 일",
  note: null,
  status: "open",
  position: 0,
  repo: null,
  task_id: null,
  source: "manual",
  source_ref: null,
  created_at: NOW_SECS,
  updated_at: NOW_SECS,
  done_at: null,
  carried_from: null,
  ...over,
});

let container: HTMLDivElement | null = null;
let root: Root | null = null;
let api: BacklogApi;

const mocked = (fn: unknown): ReturnType<typeof vi.fn> => fn as ReturnType<typeof vi.fn>;

async function render(props: Partial<Parameters<typeof BacklogSection>[0]> = {}): Promise<void> {
  await act(async () => {
    root?.render(<BacklogSection api={api} nowMs={NOW_MS} {...props} />);
    await Promise.resolve();
  });
}

function buttonMatching(pattern: RegExp): HTMLButtonElement | null {
  const buttons = [...(container?.querySelectorAll("button") ?? [])];
  return (buttons.find((b) => pattern.test(b.textContent ?? "")) as HTMLButtonElement) ?? null;
}

function byLabel<T extends HTMLElement>(label: string): T | null {
  return container?.querySelector<T>(`[aria-label="${label}"]`) ?? null;
}

async function click(element: Element | null): Promise<void> {
  await act(async () => {
    element?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await Promise.resolve();
  });
}

/** 헤더를 눌러 목록을 펼친다 — 기본이 접힘이라 대부분의 검증이 이걸 먼저 거친다. */
async function expand(): Promise<void> {
  await click(buttonMatching(/펼치기/));
}

beforeEach(() => {
  api = {
    list: vi.fn().mockResolvedValue([]),
    add: vi.fn().mockResolvedValue(item({})),
    move: vi.fn().mockResolvedValue(item({})),
    remove: vi.fn().mockResolvedValue(undefined),
  };
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  container = null;
  root = null;
  vi.restoreAllMocks();
});

describe("BacklogSection", () => {
  it("백로그가 비면 아무것도 그리지 않는다", async () => {
    await render();

    // 홈에 빈 껍데기를 남기지 않는다. 진입로는 오늘 목록의 '나중에'다.
    expect(container?.textContent).toBe("");
  });

  it("백로그 레인만 조회한다", async () => {
    await render();

    expect(mocked(api.list)).toHaveBeenCalledWith("backlog");
  });

  it("기본은 접혀 있고 헤더가 건수를 말한다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 1 }), item({ id: 2, title: "또 하나" })]);

    await render();

    expect(container?.textContent).toContain("백로그");
    expect(container?.textContent).toContain("2");
    // 접힌 동안에는 항목 자체가 렌더되지 않는다.
    expect(container?.textContent).not.toContain("언젠가 할 일");
  });

  it("헤더를 누르면 펼쳐진다", async () => {
    mocked(api.list).mockResolvedValue([item({})]);

    await render();
    await expand();

    expect(container?.textContent).toContain("언젠가 할 일");
  });

  it("'오늘로'가 날짜를 지정하지 않고 옮긴다 — 기본 레인이 오늘이다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 7 })]);

    await render();
    await expand();
    await click(byLabel("언젠가 할 일 오늘로"));

    expect(mocked(api.move)).toHaveBeenCalledWith(7);
    // 옮긴 뒤 목록을 다시 읽는다 — 최초 1회 + 갱신 1회.
    expect(mocked(api.list)).toHaveBeenCalledTimes(2);
  });

  it("삭제가 항목을 지운다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 9 })]);

    await render();
    await expand();
    await click(byLabel("언젠가 할 일 삭제"));

    expect(mocked(api.remove)).toHaveBeenCalledWith(9);
  });

  it("30일 넘은 항목에 적체 일수가 붙는다", async () => {
    mocked(api.list).mockResolvedValue([
      item({ id: 1, title: "묵은 것", created_at: NOW_SECS - DAY * 40 }),
      item({ id: 2, title: "새것", created_at: NOW_SECS - DAY * 3 }),
    ]);

    await render();
    await expand();

    expect(byLabel("40일 묵음")).not.toBeNull();
    // 최근에 담은 것에는 붙지 않는다 — 모든 줄에 숫자가 붙으면 신호가 죽는다.
    expect(byLabel("3일 묵음")).toBeNull();
  });

  it("적체 요약은 접힌 상태에서도 보인다", async () => {
    mocked(api.list).mockResolvedValue([
      item({ id: 1, created_at: NOW_SECS - DAY * 40 }),
      item({ id: 2, created_at: NOW_SECS - DAY * 100 }),
      item({ id: 3, created_at: NOW_SECS }),
    ]);

    await render();

    // 펼쳐야 보이면 아무도 안 본다.
    expect(byLabel("30일 넘게 묵은 항목 2건")).not.toBeNull();
  });

  it("백로그에 직접 담을 때도 레인과 레포가 함께 간다", async () => {
    mocked(api.list).mockResolvedValue([item({})]);

    await render({ repo: "/repo" });
    await expand();
    const input = container?.querySelector("input");
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype,
        "value",
      )?.set;
      setter?.call(input, "나중에 볼 것");
      input?.dispatchEvent(new Event("input", { bubbles: true }));
      await Promise.resolve();
    });
    await act(async () => {
      input?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      await Promise.resolve();
    });

    expect(mocked(api.add)).toHaveBeenCalledWith("나중에 볼 것", "backlog", "/repo");
  });

  it("실패를 삼키지 않고 보여준다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 3 })]);
    mocked(api.move).mockRejectedValue(new Error("옮길 수 없습니다"));

    await render();
    await expand();
    await click(byLabel("언젠가 할 일 오늘로"));

    expect(container?.textContent).toContain("옮길 수 없습니다");
  });
});
