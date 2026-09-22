import { describe, expect, it } from "vitest";
import type { Task } from "../ipc";
import { canConnect, emptyProgressView, parseProgressView, progressGraph, progressStorageKey, type ProgressEdge } from "./progress";

const task = (id: number, extra: Partial<Task> = {}): Task => ({
  id, host: "local", repo: "/repo", branch: `task-${id}`, base: "main", worktree_path: `/work/${id}`,
  instruction: `작업 ${id}`, state: "Queued", created_at: id, updated_at: id, mode: "conversation", ...extra,
});

describe("development progress graph", () => {
  it("lays out a diamond by prerequisites without changing positions when status changes", () => {
    const view = { ...emptyProgressView(), phaseByTask: { "1": "계획" }, dependencies: [
      { from: 1, to: 2 }, { from: 1, to: 3 }, { from: 2, to: 4 }, { from: 3, to: 4 },
    ] };
    const tasks = [task(4), task(2), task(1), task(3)];
    const graph = progressGraph(tasks, "local", "/repo", view);
    const positions = graph.nodes.map(({ task, x, y }) => ({ id: task.id, x, y }));
    expect(positions[0].x).toBeLessThan(positions[1].x);
    expect(positions[1].x).toBe(positions[2].x);
    expect(positions[1].y).not.toBe(positions[2].y);
    expect(positions[2].x).toBeLessThan(positions[3].x);
    expect(graph.nodes[0].phase).toBe("계획");
    expect(progressGraph(tasks.reverse().map((row) => ({ ...row, state: "Done" })), "local", "/repo", view)
      .nodes.map(({ task, x, y }) => ({ id: task.id, x, y }))).toEqual(positions);
  });

  it("scopes task IDs and continuation relationships by host and project", () => {
    const graph = progressGraph([
      task(1, { host: "remote" }), task(2, { repo: "/other" }), task(3, { resumed_from: 1 }),
      task(4), task(5, { resumed_from: 4 }),
    ], "local", "/repo", { ...emptyProgressView(), dependencies: [{ from: 2, to: 3 }] });
    expect(graph.nodes.map((node) => node.task.id)).toEqual([3, 4, 5]);
    expect(graph.edges).toEqual([{ from: 4, to: 5, kind: "continuation" }]);
  });

  it("rejects cycles, self edges and duplicates including continuation edges", () => {
    const edges: ProgressEdge[] = [{ from: 1, to: 2, kind: "continuation" }, { from: 2, to: 3, kind: "dependency" }];
    expect(canConnect(edges, 3, 1)).toBe(false);
    expect(canConnect(edges, 1, 1)).toBe(false);
    expect(canConnect(edges, 1, 2)).toBe(false);
    expect(canConnect(edges, 1, 3)).toBe(true);
    const graph = progressGraph([task(1), task(2, { resumed_from: 1 }), task(3)], "local", "/repo", {
      ...emptyProgressView(), dependencies: [{ from: 2, to: 3 }, { from: 3, to: 1 }, { from: 1, to: 2 }],
    });
    expect(graph.edges).toEqual(edges);
    expect(graph.omitted).toBe(2);
  });

  it("distinguishes review, authentication, running and offline states from completion", () => {
    const states = [task(1, { state: "Done" }), task(2, { state: "AwaitingReview" }), task(3, { state: "Finalizing" }),
      task(4, { state: "Queued", blocked_reason: "auth:codex" }), task(5, { state: "Failed" }),
      task(6, { state: "Done", stale: true }), task(7, { state: "Running", stale: true }), task(8, { state: "Discarded" })];
    expect(progressGraph(states, "local", "/repo", emptyProgressView()).counts)
      .toEqual({ total: 8, done: 1, running: 1, awaiting: 2, failed: 1, stale: 2 });
  });
});

describe("progress view storage", () => {
  it("round trips annotations with separate keys for host/project boundaries", () => {
    const view = { ...emptyProgressView(), phaseByTask: { "1": "구현" }, dependencies: [{ from: 1, to: 2 }] };
    expect(parseProgressView(JSON.stringify(view))).toEqual(view);
    expect(progressStorageKey("a:b", "c")).not.toBe(progressStorageKey("a", "b:c"));
    expect(progressStorageKey("local", "/repo")).not.toBe(progressStorageKey("remote", "/repo"));
  });

  it.each([
    "broken", JSON.stringify({ ...emptyProgressView(), version: 2 }),
    JSON.stringify({ ...emptyProgressView(), phases: ["계획", "계획"] }),
    JSON.stringify({ ...emptyProgressView(), phaseByTask: { "1": "missing" } }),
    JSON.stringify({ ...emptyProgressView(), phaseByTask: { "-1": "계획" } }),
    JSON.stringify({ ...emptyProgressView(), dependencies: [{ from: 1, to: 1 }] }),
    JSON.stringify({ ...emptyProgressView(), dependencies: [{ from: "1", to: 2 }] }),
    JSON.stringify({ ...emptyProgressView(), phases: Array.from({ length: 33 }, (_, i) => `phase-${i}`) }),
    " ".repeat(512 * 1024 + 1),
  ])("rejects malformed or oversized annotations (%#)", (value) => {
    expect(() => parseProgressView(value)).toThrow();
  });
});
