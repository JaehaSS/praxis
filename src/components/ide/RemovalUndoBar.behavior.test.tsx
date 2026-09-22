// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "../../lib/ipc";
import { RemovalUndoBar } from "./RemovalUndoBar";
import type { PendingRemoval } from "./useDeferredTaskRemoval";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const makeTask = (overrides: Partial<Task> = {}): Task => ({
  id: 11,
  host: "local",
  repo: "/workspace/alpha",
  branch: "feature/task-11",
  base: "main",
  worktree_path: "/workspace/alpha/.praxis/worktrees/task-11",
  instruction: "작업 11",
  state: "AwaitingReview",
  created_at: 11,
  updated_at: 11,
  mode: "conversation",
  ...overrides,
});

const makePending = (task: Task): PendingRemoval => ({ task, deadline: Date.now() + 10_000 });

let container: HTMLDivElement;
let root: Root;
let onUndo: ReturnType<typeof vi.fn>;

const render = async (pending: PendingRemoval[]): Promise<void> => {
  await act(async () => {
    root.render(<RemovalUndoBar pending={pending} onUndo={onUndo} />);
  });
};

beforeEach(() => {
  onUndo = vi.fn();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.clearAllMocks();
});

/**
 * 리뷰 바 "버리기"가 즉시 taskDiscard를 부르던 것을 유예 창으로 되돌린 뒤 붙은 배너.
 * 제목·부제는 상태에 따라 갈리는 removalHeadline/removalDetail을 그대로 옮겨 그린다.
 */
describe("RemovalUndoBar", () => {
  it("AwaitingReview 한 건이 유예 중이면 제목은 '버리기', 부제엔 브랜치 이름이 들어간다", async () => {
    const task = makeTask({ state: "AwaitingReview" });
    await render([makePending(task)]);

    expect(container.querySelector(".text-text")?.textContent).toBe('"작업 11" 버리기');
    expect(container.querySelector(".text-text-muted")?.textContent).toContain("feature/task-11");
    // 카운트다운 자체는 대상이 아니다 — 초 단위 표시가 있다는 것만 확인한다.
    expect(container.querySelector(".text-text-secondary")?.textContent).toMatch(/^\d+초$/);
  });

  it("직접 실행(repo === worktree_path)인 AwaitingReview는 워크트리를 지운다고 말하지 않는다", async () => {
    const task = makeTask({
      state: "AwaitingReview",
      repo: "/workspace/alpha",
      worktree_path: "/workspace/alpha",
    });
    await render([makePending(task)]);

    const detail = container.querySelector(".text-text-muted")?.textContent ?? "";
    expect(detail).toContain("직접 실행");
    expect(detail).not.toContain("워크트리를 정리합니다");
  });

  it("종료 상태(Done) 한 건은 제목이 '삭제'로 읽힌다 — 버리기가 아니다", async () => {
    const task = makeTask({ state: "Done" });
    await render([makePending(task)]);

    expect(container.querySelector(".text-text")?.textContent).toBe('"작업 11" 삭제');
  });

  it("실행 취소 버튼을 누르면 onUndo가 불린다", async () => {
    const task = makeTask();
    await render([makePending(task)]);

    const button = container.querySelector("button");
    expect(button).not.toBeNull();
    await act(async () => {
      button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(onUndo).toHaveBeenCalledOnce();
  });

  it("pending이 비면 아무것도 렌더링하지 않는다", async () => {
    await render([]);

    expect(container.innerHTML).toBe("");
  });
});
