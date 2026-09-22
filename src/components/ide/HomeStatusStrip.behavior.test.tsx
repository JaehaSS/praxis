// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";

import { HomeStatusStrip } from "./HomeStatusStrip";
import type { Task } from "../../lib/ipc";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const task = (over: Partial<Task> & { id: number }): Task => ({
  host: "local",
  repo: "/workspace/praxis",
  branch: `task-${over.id}`,
  base: "main",
  worktree_path: `/tmp/task-${over.id}`,
  instruction: `작업 ${over.id}`,
  state: "Running",
  created_at: over.id,
  updated_at: over.id,
  mode: "conversation",
  ...over,
});

let container: HTMLDivElement | null = null;
let root: Root | null = null;

async function render(tasks: Task[]) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root?.render(<HomeStatusStrip tasks={tasks} />);
  });
  return container;
}

/** 라벨이 붙은 칸 — 칸 순서에 기대지 않고 읽는다. */
const cell = (el: HTMLElement, label: string): HTMLElement | null => {
  const labelNode = [...el.querySelectorAll("span")].find((s) => s.textContent === label);
  return (labelNode?.parentElement as HTMLElement | undefined) ?? null;
};

const cellValue = (el: HTMLElement, label: string): string | null =>
  cell(el, label)?.querySelector("[data-cell-value]")?.textContent ?? null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  container = null;
  root = null;
});

describe("HomeStatusStrip", () => {
  it("작업 상태를 네 칸의 숫자로 세운다", async () => {
    const el = await render([
      task({ id: 1, state: "Running" }),
      task({ id: 2, state: "Starting" }),
      task({ id: 3, state: "AwaitingReview" }),
      task({ id: 4, state: "Queued" }),
    ]);

    expect(cellValue(el, "도는 중")).toBe("2");
    expect(cellValue(el, "검토 대기")).toBe("1");
    expect(cellValue(el, "승인 대기")).toBe("0");
    expect(cellValue(el, "오늘 완료")).toBe("0");
  });

  it("실행 승인 대기는 승인 대기 칸에만 선다 — 검토 대기와 겹쳐 세지 않는다", async () => {
    const el = await render([
      task({ id: 1, state: "PendingApproval" }),
      task({ id: 2, state: "AwaitingReview" }),
    ]);

    expect(cellValue(el, "승인 대기")).toBe("1");
    expect(cellValue(el, "검토 대기")).toBe("1");
  });

  it("작업이 하나도 없어도 네 칸이 그대로 선다 — 스트립 자리가 흔들리지 않는다", async () => {
    const el = await render([]);

    expect(el.querySelector("section")?.getAttribute("aria-label")).toBe("현황 요약");
    expect(el.querySelectorAll("[data-cell-value]").length).toBe(4);
    expect(cellValue(el, "도는 중")).toBe("0");
    expect(cellValue(el, "검토 대기")).toBe("0");
    expect(cellValue(el, "승인 대기")).toBe("0");
    expect(cellValue(el, "오늘 완료")).toBe("0");
  });
});
