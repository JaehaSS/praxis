// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TodaySection, type TodayApi } from "./TodaySection";
import type { DayItem, DaySuggestion } from "./today-items";
import type { Task } from "../../lib/ipc";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const item = (over: Partial<DayItem>): DayItem => ({
  id: 1,
  day: "2026-08-03",
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

const task = (over: Partial<Task>): Task =>
  ({
    id: 42,
    repo: "/r",
    branch: "b",
    base: "main",
    worktree_path: "/w",
    instruction: "i",
    state: "Running",
    created_at: 0,
    updated_at: 0,
    mode: "conversation",
    ...over,
  }) as Task;

const suggestion = (over: Partial<DaySuggestion>): DaySuggestion => ({
  title: "제안",
  source: "awaiting",
  source_ref: "1",
  repo: null,
  ...over,
});

let container: HTMLDivElement | null = null;
let root: Root | null = null;
let api: TodayApi;

/** 항상 vi.fn()이므로 캐스팅 헬퍼로 단언 소음을 줄인다. */
const mocked = (fn: unknown): ReturnType<typeof vi.fn> => fn as ReturnType<typeof vi.fn>;

async function render(props: Partial<Parameters<typeof TodaySection>[0]> = {}): Promise<void> {
  await act(async () => {
    root?.render(<TodaySection api={api} {...props} />);
    await Promise.resolve();
  });
}

function query<T extends HTMLElement>(selector: string): T | null {
  return container?.querySelector<T>(selector) ?? null;
}

function buttonByText(text: string): HTMLButtonElement | null {
  const buttons = [...(container?.querySelectorAll("button") ?? [])];
  return (buttons.find((b) => b.textContent?.trim() === text) as HTMLButtonElement) ?? null;
}

function buttonMatching(pattern: RegExp): HTMLButtonElement | null {
  const buttons = [...(container?.querySelectorAll("button") ?? [])];
  return (buttons.find((b) => pattern.test(b.textContent ?? "")) as HTMLButtonElement) ?? null;
}

async function click(element: Element | null): Promise<void> {
  await act(async () => {
    element?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await Promise.resolve();
  });
}

/** React 제어 input에 값을 넣는다 — value setter를 우회하면 onChange가 안 불린다. */
async function type(input: HTMLInputElement, value: string): Promise<void> {
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value",
    )?.set;
    setter?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await Promise.resolve();
  });
}

async function pressEnter(input: HTMLInputElement): Promise<void> {
  await act(async () => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await Promise.resolve();
  });
}

beforeEach(() => {
  api = {
    list: vi.fn().mockResolvedValue([]),
    add: vi.fn().mockResolvedValue(item({})),
    setStatus: vi.fn().mockResolvedValue(item({})),
    remove: vi.fn().mockResolvedValue(undefined),
    move: vi.fn().mockResolvedValue(item({})),
    reorder: vi.fn().mockResolvedValue(undefined),
    start: vi.fn().mockResolvedValue(task({})),
    suggest: vi.fn().mockResolvedValue([]),
    take: vi.fn().mockResolvedValue(item({})),
    close: vi.fn().mockResolvedValue({
      day: "2026-08-03",
      closed_at: 0,
      done: 1,
      open: 2,
      dropped: 0,
      draft:
        "### #N · 2026-08-03 · <제목> (Small)\n\n- **subject**: <키>\n- **status**: done\n",
    }),
  };
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.clearAllMocks();
});

describe("TodaySection — 입력", () => {
  it("Enter 한 번으로 항목이 추가된다 — 별도 버튼을 누르지 않아도 된다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 1 })]);
    await render();
    const input = query<HTMLInputElement>("input[placeholder]")!;
    await type(input, "새 항목");
    await pressEnter(input);
    expect(api.add).toHaveBeenCalledWith("새 항목", undefined, undefined);
  });

  it("빈 입력에 Enter를 눌러도 호출하지 않는다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 1 })]);
    await render();
    const input = query<HTMLInputElement>("input[placeholder]")!;
    await type(input, "   ");
    await pressEnter(input);
    expect(api.add).not.toHaveBeenCalled();
  });
});

describe("TodaySection — 상태 전이", () => {
  it("체크박스를 누르면 done으로 전이한다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 5, title: "할 일" })]);
    await render();
    await click(query('input[type="checkbox"]'));
    expect(api.setStatus).toHaveBeenCalledWith(5, "done");
  });

  it("이미 done인 항목을 누르면 open으로 되돌린다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 5, title: "할 일", status: "done" })]);
    await render();
    await click(query('input[type="checkbox"]'));
    expect(api.setStatus).toHaveBeenCalledWith(5, "open");
  });

  it("dropped 항목은 취소선으로 그리고 진행률에서 뺀다", async () => {
    mocked(api.list).mockResolvedValue([
      item({ id: 1, title: "함", status: "done" }),
      item({ id: 2, title: "접음", status: "dropped", position: 1 }),
    ]);
    await render();
    expect(container?.textContent).toContain("1/1");
    const dropped = [...(container?.querySelectorAll("span") ?? [])].find(
      (s) => s.textContent === "접음",
    );
    expect(dropped?.className).toContain("line-through");
  });
});

describe("TodaySection — 이월", () => {
  it("넘어온 항목은 어디서 왔는지 밝힌다", async () => {
    mocked(api.list).mockResolvedValue([
      item({ id: 1, title: "밀린 것", day: "2026-08-03", carried_from: "2026-08-02" }),
    ]);
    await render();
    expect(query('[aria-label="어제에서 넘어온 항목"]')).not.toBeNull();
  });

  it("오늘 정한 항목에는 표시가 붙지 않는다 — 대부분의 줄이 조용해야 한다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 1, title: "오늘 것" })]);
    await render();
    expect(container?.textContent).not.toContain("↩");
  });

  // 밀린 항목을 정리할지는 사용자가 판단한다 — 경고색으로 압박하지 않는다 (설계 0021 Changelog).
  it("이월 표시는 경고가 아니다", async () => {
    mocked(api.list).mockResolvedValue([
      item({ id: 1, day: "2026-08-03", carried_from: "2026-07-20" }),
    ]);
    await render();
    const badge = query('[aria-label="7/20에서 넘어온 항목"]');
    expect(badge?.className).toContain("text-text-muted");
    expect(badge?.className).not.toContain("status-failed");
  });
});

describe("TodaySection — 착수", () => {
  it("repo가 없는 항목의 착수 버튼은 비활성이다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 5, title: "문서 쓰기", repo: null })]);
    await render();
    expect(buttonByText("착수")?.disabled).toBe(true);
  });

  it("repo가 있으면 착수할 수 있다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 5, title: "구현", repo: "/r" })]);
    await render();
    await click(buttonByText("착수"));
    expect(api.start).toHaveBeenCalledWith(5, "claude", "conversation");
  });

  it("이미 착수한 항목은 Task 상태 배지를 보이고 재착수를 막는다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 5, repo: "/r", task_id: 42 })]);
    await render({ tasks: [task({ id: 42, state: "Running" })] });
    expect(query('[aria-label="작업 42"]')).not.toBeNull();
    expect(buttonByText("착수")?.disabled).toBe(true);
  });
});

describe("TodaySection — 제안", () => {
  it("담기를 누르면 take를 호출한다", async () => {
    const s = suggestion({ title: "어제 못 한 것" });
    mocked(api.suggest).mockResolvedValue([s]);
    await render();
    await click(buttonMatching(/제안 1건/));
    await click(buttonByText("담기"));
    expect(api.take).toHaveBeenCalledWith(s);
  });

  it("제안은 접힌 채로 시작한다 — 담기 전까지 목록에 섞이지 않는다", async () => {
    mocked(api.suggest).mockResolvedValue([suggestion({ title: "어제 못 한 것" })]);
    await render();
    expect(buttonMatching(/제안 1건/)).not.toBeNull();
    expect(container?.textContent).not.toContain("어제 못 한 것");
  });
});

describe("TodaySection — 빈 상태", () => {
  it("항목 0건 + 제안 0건이면 진입로 버튼 하나만 남긴다 — 섹션 껍데기는 안 그린다", async () => {
    await render();
    expect(api.list).toHaveBeenCalled();
    // 진입로는 있다 (없으면 첫 실행이 곧 영구 미사용이 된다)
    expect(buttonMatching(/\+ 오늘 할 일/)).not.toBeNull();
    // 그러나 섹션·입력창·진행률은 없다
    expect(container?.querySelector("section")).toBeNull();
    expect(container?.querySelector("input")).toBeNull();
    expect(container?.querySelectorAll("button")).toHaveLength(1);
  });

  it("진입로를 누르면 입력창이 열린다", async () => {
    await render();
    await click(buttonMatching(/\+ 오늘 할 일/));
    const input = query<HTMLInputElement>("input[placeholder]");
    expect(input).not.toBeNull();
    // 열린 뒤에도 항목이 없으므로 빈 목록 테두리는 그리지 않는다
    expect(container?.querySelector(".border-border-strong")).toBeNull();
  });

  it("진입로로 연 뒤 항목을 추가할 수 있다", async () => {
    await render();
    await click(buttonMatching(/\+ 오늘 할 일/));
    const input = query<HTMLInputElement>("input[placeholder]")!;
    await type(input, "첫 항목");
    await pressEnter(input);
    expect(api.add).toHaveBeenCalledWith("첫 항목", undefined, undefined);
  });

  it("항목은 없어도 제안이 있으면 진입로 없이 바로 제안 진입점을 보인다", async () => {
    mocked(api.suggest).mockResolvedValue([suggestion({})]);
    await render();
    expect(buttonMatching(/제안 1건/)).not.toBeNull();
    expect(buttonMatching(/\+ 오늘 할 일/)).toBeNull();
  });
});

describe("TodaySection — 채널 밀도", () => {
  const renderChannel = (over: Partial<Parameters<typeof TodaySection>[0]> = {}) =>
    render({ density: "channel", ...over });

  it("제안을 조회하지 않는다 — gh를 타는 왕복을 세션 화면에서 반복하지 않는다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 1 })]);
    await renderChannel({ repo: "/a" });
    expect(api.suggest).not.toHaveBeenCalled();
    expect(api.list).toHaveBeenCalled();
  });

  it("항목을 카드로 그리고 체크로 완료 처리한다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 1, title: "리뷰 반영" })]);
    await renderChannel();

    expect(query('aside[aria-label="오늘 할 일"]')).not.toBeNull();
    const checkbox = query<HTMLInputElement>('input[aria-label="리뷰 반영"]')!;
    await click(checkbox);
    expect(api.setStatus).toHaveBeenCalledWith(1, "done");
  });

  it("정렬·삭제·착수·마감은 두지 않는다 — 정본 편집은 Home이 맡는다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 1, title: "리뷰 반영", repo: "/a" })]);
    await renderChannel();

    expect(buttonByText("착수")).toBeNull();
    expect(buttonByText("하루 마감")).toBeNull();
    expect(query('[aria-label="리뷰 반영 삭제"]')).toBeNull();
    expect(query('[aria-label="리뷰 반영 위로"]')).toBeNull();
  });

  it("세션 중 떠오른 일을 한 줄로 담는다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 1 })]);
    await renderChannel({ repo: "/a" });
    const input = query<HTMLInputElement>("input[placeholder]")!;
    await type(input, "회귀 테스트 추가");
    await pressEnter(input);
    expect(api.add).toHaveBeenCalledWith("회귀 테스트 추가", undefined, "/a");
  });

  it("비면 진입로 한 줄만 남긴다 — 세션 위에 빈 껍데기를 띄우지 않는다", async () => {
    await renderChannel();
    const card = query('aside[aria-label="오늘 할 일"]');
    expect(card).not.toBeNull();
    expect(card?.querySelector("input")).toBeNull();

    await click(buttonMatching(/\+ 오늘 할 일/));
    expect(query<HTMLInputElement>("input[placeholder]")).not.toBeNull();
  });
});

describe("TodaySection — 하루 마감", () => {
  it("마감하면 집계와 원장 초안을 보이되 원장에 쓰지는 않는다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 1 })]);
    await render();
    await click(buttonByText("하루 마감"));
    expect(query('[role="dialog"]')).not.toBeNull();
    const draft = query<HTMLTextAreaElement>('textarea[aria-label="원장 초안"]')!;
    expect(draft.readOnly).toBe(true);
    expect(draft.value).toContain("- **subject**:");
    expect(draft.value).toContain("- **status**:");
  });
});

describe("백로그로 미루기", () => {
  it("'나중에'가 항목을 백로그 레인으로 옮긴다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 5, title: "오늘은 아니다" })]);

    await render();
    await click(container?.querySelector('[aria-label="오늘은 아니다 나중에"]') ?? null);

    expect(mocked(api.move)).toHaveBeenCalledWith(5, "backlog");
  });

  it("끝난 항목에는 '나중에'가 없다", async () => {
    mocked(api.list).mockResolvedValue([
      item({ id: 1, title: "한 일", status: "done" }),
      item({ id: 2, title: "접은 일", status: "dropped" }),
    ]);

    await render();

    // 끝난 결정을 미결로 되돌리는 경로다. 되살리려면 체크박스로 먼저 open으로 돌린다.
    expect(container?.querySelector('[aria-label="한 일 나중에"]')).toBeNull();
    expect(container?.querySelector('[aria-label="접은 일 나중에"]')).toBeNull();
  });

  it("세션 채널에는 백로그 손잡이가 없다", async () => {
    mocked(api.list).mockResolvedValue([item({ id: 5, title: "오늘은 아니다" })]);

    await render({ density: "channel" });

    // 채널은 참조·체크용 압축본이다 — 레인 이동은 정본인 홈에만 둔다.
    expect(container?.querySelector('[aria-label="오늘은 아니다 나중에"]')).toBeNull();
  });
});
