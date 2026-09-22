import { describe, expect, it } from "vitest";
import type { Task } from "../../lib/ipc";
import { recentItems } from "./home-items";

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

describe("recentItems", () => {
  it("앙상블 후보는 흩어지지 않고 그룹 한 행으로 접힌다", () => {
    const items = recentItems([
      task({ id: 1, ensemble: "e1", agent: "claude" }),
      task({ id: 2, ensemble: "e1", agent: "codex" }),
      task({ id: 3, ensemble: "e1", agent: "gemini" }),
      task({ id: 4 }),
    ]);

    expect(items).toHaveLength(2);
    const group = items.find((i) => i.kind === "ensemble");
    expect(group).toMatchObject({ kind: "ensemble", id: "e1" });
    expect(group?.kind === "ensemble" && group.tasks.map((t) => t.id)).toEqual([1, 2, 3]);
  });

  it("그룹의 정렬 시각은 가장 최근 후보 — 오래된 후보 때문에 목록 아래로 가라앉지 않는다", () => {
    const items = recentItems([
      task({ id: 1, ensemble: "e1", created_at: 10 }),
      task({ id: 2, ensemble: "e1", created_at: 90 }),
      task({ id: 3, created_at: 50 }),
    ]);

    expect(items.map((i) => i.key)).toEqual(["ensemble:e1", "task:3"]);
    expect(items[0]?.at).toBe(90);
  });

  it("ready는 자율수행이 끝난 후보만 센다", () => {
    const items = recentItems([
      task({ id: 1, ensemble: "e1", state: "Done" }),
      task({ id: 2, ensemble: "e1", state: "AwaitingReview" }),
      task({ id: 3, ensemble: "e1", state: "Running" }),
      task({ id: 4, ensemble: "e1", state: "Failed" }),
    ]);

    const group = items[0];
    expect(group?.kind === "ensemble" && group.ready).toBe(2);
    expect(group?.kind === "ensemble" && group.tasks).toHaveLength(4);
  });

  it("최신순으로 자르며, 접힌 그룹은 한 자리만 차지한다", () => {
    const tasks = [
      task({ id: 1, ensemble: "e1", created_at: 100 }),
      task({ id: 2, ensemble: "e1", created_at: 101 }),
      ...[3, 4, 5, 6, 7, 8].map((id) => task({ id, created_at: id })),
    ];

    const items = recentItems(tasks, 3);
    expect(items.map((i) => i.key)).toEqual(["ensemble:e1", "task:8", "task:7"]);
  });

  it("같은 시각이면 최근 id가 먼저 — 순서가 렌더마다 흔들리지 않는다", () => {
    const items = recentItems([
      task({ id: 5, created_at: 42 }),
      task({ id: 9, created_at: 42 }),
      task({ id: 7, created_at: 42 }),
    ]);

    expect(items.map((i) => i.key)).toEqual(["task:9", "task:7", "task:5"]);
  });

  it("앙상블이 없으면 단일 작업만 최신순으로 돌려준다", () => {
    const items = recentItems([task({ id: 1 }), task({ id: 2 })]);
    expect(items.map((i) => i.kind)).toEqual(["task", "task"]);
    expect(items.map((i) => i.key)).toEqual(["task:2", "task:1"]);
  });

  it("완료·버림 단일 작업과 그런 후보만 든 앙상블은 최근에서 뺀다", () => {
    const items = recentItems([
      task({ id: 1, state: "Done" }),
      task({ id: 2, state: "Discarded" }),
      task({ id: 3, ensemble: "settled", state: "Done" }),
      task({ id: 4, ensemble: "settled", state: "Discarded" }),
      task({ id: 5, ensemble: "mixed", state: "Done" }),
      task({ id: 6, ensemble: "mixed", state: "Running" }),
    ]);

    expect(items.map((item) => item.key)).toEqual(["ensemble:mixed"]);
    expect(items[0]?.kind === "ensemble" && items[0].tasks.map((member) => member.id)).toEqual([5, 6]);
  });
});
