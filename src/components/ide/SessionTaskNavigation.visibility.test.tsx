// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "../../lib/ipc";
import { SessionTaskNavigation } from "./SessionTaskNavigation";

const notifications = vi.hoisted(() => ({
  snapshot: null as { items: Array<{ host: string; task_id: number }> } | null,
}));

vi.mock("../../lib/use-notification-snapshot", () => ({
  useNotificationSnapshot: () => ({ snapshot: notifications.snapshot }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const repo = "/workspace/praxis";

const task = (id: number, state: string, extra: Partial<Task> = {}): Task => ({
  id,
  host: "local",
  repo,
  branch: `task-${id}`,
  base: "main",
  worktree_path: `/tmp/task-${id}`,
  instruction: `작업 ${id}`,
  state,
  created_at: id,
  updated_at: id,
  mode: "conversation",
  ...extra,
});

let container: HTMLDivElement;
let root: Root;

async function render(tasks: Task[]): Promise<string> {
  await act(async () => {
    root.render(
      <SessionTaskNavigation
        tasks={tasks}
        selectedKey={null}
        projects={[repo]}
        onOpenTask={() => {}}
        onNewInRepo={() => {}}
        onDeleteTask={() => {}}
        onRemoveProject={() => {}}
        onDiscardOrphans={() => {}}
      />,
    );
  });
  return container.innerHTML;
}

describe("SessionTaskNavigation visibility", () => {
  beforeEach(() => {
    notifications.snapshot = null;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
  });

  it("Done·Discarded는 stale·unread여도 제외하고 AwaitingReview·Finalizing과 Failed는 남긴다", async () => {
    notifications.snapshot = { items: [
      { host: "local", task_id: 1 },
      { host: "local", task_id: 2 },
      { host: "local", task_id: 3 },
    ] };
    const html = await render([
      task(1, "Done", { stale: true }),
      task(2, "Discarded"),
      task(3, "Failed"),
      task(4, "AwaitingReview"),
      task(5, "Finalizing"),
      task(1, "Running", { host: "runner-1", instruction: "원격 활성" }),
    ]);

    expect(html).not.toContain("작업 1");
    expect(html).not.toContain("작업 2");
    expect(html).toContain("작업 3");
    expect(html).toContain("작업 4");
    expect(html).toContain("작업 5");
    expect(html).toContain("원격 활성");
  });

  it("같은 mounted navigation을 새 목록으로 갱신해도 완료된 작업은 돌아오지 않는다", async () => {
    expect(await render([task(1, "Running")])).toContain("작업 1");
    expect(await render([task(1, "Done", { stale: true })])).not.toContain("작업 1");
  });
});
