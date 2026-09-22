import { describe, expect, it } from "vitest";
import { mergeTaskLists, type HostTaskResult } from "./task-list-merge";
import type { Task } from "./ipc";

const task = (host: string, id: number, updated_at: number): Task => ({
  host,
  id,
  repo: "/repo",
  branch: `task-${id}`,
  base: "main",
  worktree_path: `/wt/${id}`,
  instruction: `작업 ${id}`,
  state: "Running",
  created_at: 0,
  updated_at,
  mode: "conversation",
});

const ok = (host: string, tasks: Task[]): HostTaskResult => ({ host, tasks, error: null });
const failed = (host: string, error: string): HostTaskResult => ({ host, tasks: null, error });

describe("mergeTaskLists", () => {
  it("두 호스트의 작업을 updated_at 내림차순으로 섞는다", () => {
    const merged = mergeTaskLists([
      ok("local", [task("local", 1, 100), task("local", 2, 300)]),
      ok("mini1", [task("mini1", 5, 200)]),
    ]);

    expect(merged.tasks.map((t) => [t.host, t.id])).toEqual([
      ["local", 2],
      ["mini1", 5],
      ["local", 1],
    ]);
    expect(merged.failures).toEqual([]);
  });

  it("같은 id라도 호스트가 다르면 둘 다 살아남는다", () => {
    const merged = mergeTaskLists([
      ok("local", [task("local", 3, 100)]),
      ok("mini1", [task("mini1", 3, 90)]),
    ]);

    expect(merged.tasks).toHaveLength(2);
    expect(merged.tasks.map((t) => t.host)).toEqual(["local", "mini1"]);
  });

  it("한 호스트가 실패해도 나머지는 반환되고 실패 호스트가 함께 나온다", () => {
    const merged = mergeTaskLists([
      ok("local", [task("local", 1, 10)]),
      failed("mini1", "Runner 요청 실패 (502)"),
    ]);

    // 원격이 죽었다고 로컬 작업까지 감추면 안 된다 — 그게 이 병합의 요지다.
    expect(merged.tasks.map((t) => t.host)).toEqual(["local"]);
    expect(merged.failures).toEqual([{ host: "mini1", error: "Runner 요청 실패 (502)" }]);
  });

  it("실패 사유가 없으면 그 자리에 기본 문구를 채운다", () => {
    const merged = mergeTaskLists([{ host: "mini1", tasks: null, error: null }]);

    expect(merged.failures).toEqual([{ host: "mini1", error: "응답 없음" }]);
  });

  it("모든 호스트가 실패하면 빈 목록과 실패 전부를 준다", () => {
    const merged = mergeTaskLists([failed("local", "a"), failed("mini1", "b")]);

    expect(merged.tasks).toEqual([]);
    expect(merged.failures).toHaveLength(2);
  });

  it("실패한 호스트에는 마지막 성공 목록을 offline 행으로 남긴다", () => {
    const cached = [task("mini1", 8, 80)];
    const merged = mergeTaskLists([failed("mini1", "offline")], () => cached.map((row) => ({ ...row, stale: true })));

    expect(merged.tasks).toEqual([{ ...cached[0], stale: true }]);
    expect(merged.failures).toEqual([{ host: "mini1", error: "offline" }]);
  });

  it("연결을 기대하지 않는 호스트(dormant)는 캐시만 합치고 실패로 알리지 않는다", () => {
    const cached = [task("mini1", 8, 80)];
    const merged = mergeTaskLists(
      [{ host: "mini1", tasks: null, error: "연결 안 됨", dormant: true }],
      () => cached,
    );

    // 저장만 된 프로필이 안 붙은 것은 오류가 아니다 — 카드 없이 마지막 목록만 남긴다.
    expect(merged.tasks).toEqual(cached);
    expect(merged.failures).toEqual([]);
  });

  it("정상 빈 응답은 캐시를 쓰지 않는다", () => {
    const merged = mergeTaskLists([ok("mini1", [])], () => [task("mini1", 8, 80)]);

    expect(merged.tasks).toEqual([]);
  });
});
