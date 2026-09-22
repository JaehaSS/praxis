// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { TaskListCache } from "./task-list-cache";
import type { Task } from "./ipc";

const task = (id: number): Task => ({
  host: "worker", id, repo: "/repo", branch: "main", base: "main", worktree_path: "/repo",
  instruction: "cached title", state: "Done", created_at: 1, updated_at: Math.floor(Date.now() / 1000), mode: "conversation",
});

beforeEach(() => localStorage.clear());

describe("TaskListCache", () => {
  it("keeps a failed host's last successful rows as stale", () => {
    const cache = new TaskListCache();
    cache.save("worker", [task(3)]);

    expect(cache.load("worker")).toEqual([{
      host: "worker", id: 3, repo: "/repo", instruction: "cached title", branch: "", base: "",
      worktree_path: "", state: "Done", created_at: task(3).updated_at, updated_at: task(3).updated_at,
      mode: "conversation", stale: true,
    }]);
  });

  it("clears a host cache after a successful empty list", () => {
    const cache = new TaskListCache();
    cache.save("worker", [task(3)]);
    cache.save("worker", []);

    expect(cache.load("worker")).toEqual([]);
  });

  it("stores only metadata and ignores corrupt host values", () => {
    localStorage.setItem("praxis-task-list-cache-v1", JSON.stringify({ worker: {}, bad: [{ id: "no" }] }));
    const cache = new TaskListCache();
    cache.save("worker", [{ ...task(3), goal_contract: { secret: "no" } } as unknown as Task]);

    const raw = localStorage.getItem("praxis-task-list-cache-v1") ?? "";
    expect(raw).not.toContain("goal_contract");
    expect(raw).not.toContain("secret");
    expect(cache.load("bad")).toEqual([]);
  });
});
