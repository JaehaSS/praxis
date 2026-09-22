import { describe, expect, it } from "vitest";
import type { Task } from "./ipc";
import { EMPTY_PROJECT_GROUPS, type ProjectGroups } from "./project-groups";
import {
  buildSessionNavigatorTree,
  filterSessionNavigatorTree,
  sessionNavigatorRows,
} from "./session-navigator";

const project = "/workspace/alpha";
const otherProject = "/workspace/beta";

const task = (id: number, host = "local", repo = project): Task => ({
  id,
  host,
  repo,
  branch: `branch-${host}-${id}`,
  base: "main",
  worktree_path: repo,
  instruction: `session-${host}-${id}`,
  state: "Running",
  created_at: id,
  updated_at: id,
  mode: "conversation",
});

describe("session navigator tree", () => {
  it("keeps same numeric IDs on different hosts as separate session rows", () => {
    const rows = sessionNavigatorRows(
      buildSessionNavigatorTree([project], [task(7), task(7, "remote")], EMPTY_PROJECT_GROUPS),
      new Set(),
      false,
    );

    expect(rows.filter((row) => row.kind === "session").map((row) => row.id)).toEqual([
      "session:local:7",
      "session:remote:7",
    ]);
  });

  it("reveals a session and its collapsed ancestors while searching", () => {
    const groups: ProjectGroups = {
      version: 1,
      groups: [{ id: "team", name: "Team", collapsed: true }],
      assignment: { [project]: "team" },
    };
    const filtered = filterSessionNavigatorTree(
      buildSessionNavigatorTree([project], [task(1)], groups),
      "session-local-1",
    );

    expect(sessionNavigatorRows(filtered, new Set(["group:team", `project:${project}`]), true)
      .map((row) => row.label)).toEqual(["Team", "alpha", "session-local-1"]);
  });

  it("keeps empty groups and unassigned projects reachable", () => {
    const groups: ProjectGroups = {
      version: 1,
      groups: [{ id: "empty", name: "Empty", collapsed: false }],
      assignment: { [project]: "empty" },
    };
    const rows = sessionNavigatorRows(
      buildSessionNavigatorTree([project, otherProject], [], groups),
      new Set(),
      false,
    );

    expect(rows.map((row) => row.label)).toEqual([
      "Empty", "alpha", "세션 없음", "미소속 프로젝트", "beta", "세션 없음",
    ]);
  });
});
