// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "../../lib/ipc";

const mocks = vi.hoisted(() => ({
  proposalRefine: vi.fn(async (): Promise<number | null> => 1),
  // ActivityPanel이 품은 WorkContextPanel이 마운트 시 호출한다 — 모듈 전체를 대체하므로
  // 직접 쓰지 않는 export도 채워야 한다(#17에서 추가된 뒤 이 mock이 뒤처져 있었다).
  taskToolCost: vi.fn(async () => null),
}));

vi.mock("../../lib/ipc", () => ({
  proposalRefine: mocks.proposalRefine,
  taskToolCost: mocks.taskToolCost,
}));

import { ActivityPanel } from "./ActivityPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const task: Task = {
  id: 7,
  host: "local",
  repo: "/workspace/praxis",
  branch: "feature/refine",
  base: "main",
  worktree_path: "/workspace/praxis/.praxis/worktrees/task-7",
  instruction: "회고 버튼",
  state: "Running",
  created_at: 1,
  updated_at: 2,
  mode: "conversation",
};

let container: HTMLDivElement;
let root: Root;

const render = (busy: boolean) => {
  act(() => {
    root.render(
      <ActivityPanel
        task={task}
        runtime="local"
        diff={{ state: "ready", value: "1 file changed" }}
        items={[]}
        busy={busy}
        activity={null}
        onRefreshDiff={() => {}}
        onOpenSubagent={() => {}}
      />,
    );
  });
};

const refineButton = (): HTMLButtonElement => {
  const found = [...container.querySelectorAll("button")].find((b) =>
    b.textContent?.includes("회고"),
  );
  if (!found) throw new Error("회고 버튼을 찾지 못했습니다");
  return found as HTMLButtonElement;
};

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  mocks.proposalRefine.mockClear();
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("ActivityPanel 회고 버튼", () => {
  it("에이전트가 작업 중이면 누를 수 없다", () => {
    render(true);
    expect(refineButton().disabled).toBe(true);
  });

  it("현재 작업 id로 회고를 호출하고 결과를 알린다", async () => {
    render(false);
    await act(async () => {
      refineButton().click();
    });

    expect(mocks.proposalRefine).toHaveBeenCalledWith(7);
    expect(container.textContent).toContain("제안이 올라왔습니다");
  });

  it("회고할 내용이 없으면 제안이 생긴 것처럼 보이지 않는다", async () => {
    mocks.proposalRefine.mockResolvedValueOnce(null);
    render(false);
    await act(async () => {
      refineButton().click();
    });

    expect(container.textContent).toContain("회고할 대화 내용이 없습니다");
    expect(container.textContent).not.toContain("제안이 올라왔습니다");
  });

  it("실패는 삼키지 않고 그대로 보여준다", async () => {
    mocks.proposalRefine.mockRejectedValueOnce(new Error("claude 없음"));
    render(false);
    await act(async () => {
      refineButton().click();
    });

    expect(container.textContent).toContain("claude 없음");
  });
});
