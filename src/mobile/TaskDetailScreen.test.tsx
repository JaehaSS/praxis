// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "../lib/ipc";

const mocks = vi.hoisted(() => ({
  taskList: vi.fn(),
  taskDiff: vi.fn(),
  taskApprove: vi.fn(),
  taskDiscard: vi.fn(),
  subscribeEvents: vi.fn(() => () => {}),
}));

vi.mock("./api", () => ({ api: mocks }));

import { TaskDetailScreen } from "./TaskDetailScreen";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const task = (over: Partial<Task> = {}): Task => ({
  id: 7,
  host: "local",
  repo: "/workspace/praxis",
  branch: "praxis/task-7",
  base: "main",
  worktree_path: "/workspace/praxis/.praxis/worktrees/task-7",
  instruction: "승인 대상 확인",
  state: "AwaitingReview",
  created_at: 1,
  updated_at: 1,
  mode: "conversation",
  ...over,
});

let container: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  mocks.taskDiff.mockResolvedValue({ files: [] });
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.clearAllMocks();
});

async function renderTask(value: Task): Promise<void> {
  mocks.taskList.mockResolvedValue([value]);
  await act(async () => {
    root?.render(<TaskDetailScreen id={value.id} tab="review" />);
    await Promise.resolve();
  });
}

function buttonWithText(text: string): HTMLButtonElement | null {
  return (
    [...(container?.querySelectorAll("button") ?? [])].find((button) => button.textContent?.trim() === text) as
      | HTMLButtonElement
      | undefined
  ) ?? null;
}

describe("TaskDetailScreen approval copy", () => {
  it("격리 작업은 저장된 base를 승인 CTA와 확인문에 보인다", async () => {
    await renderTask(task({ base: "dev" }));

    const approve = buttonWithText("dev에 승인하고 머지");
    expect(approve).not.toBeNull();
    await act(async () => approve?.click());

    expect(container?.textContent).toContain("변경을 dev 브랜치에 머지합니다.");
  });

  it("직접 작업은 base SHA를 머지 대상으로 보이지 않는다", async () => {
    const base = "a1b2c3d4";
    await renderTask(task({ base, worktree_path: "/workspace/praxis" }));

    const approve = buttonWithText("승인");
    expect(approve).not.toBeNull();
    expect(container?.textContent).not.toContain(base);
    expect(container?.textContent).not.toContain("머지");
    await act(async () => approve?.click());

    expect(container?.textContent).toContain("변경을 승인합니다.");
    expect(container?.textContent).not.toContain(base);
    expect(container?.textContent).not.toContain("머지");
  });
});
