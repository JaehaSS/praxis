import { describe, expect, it } from "vitest";
import type { Task } from "../../lib/ipc";
import { homeStats } from "./home-stats";

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

/** 자정 경계를 고정하려고 시각을 주입한다 — 실행 시점에 따라 결과가 흔들리면 안 된다. */
const NOON = new Date(2026, 7, 21, 12, 0, 0);
const secs = (d: Date) => Math.floor(d.getTime() / 1000);
const MIDNIGHT = secs(new Date(2026, 7, 21, 0, 0, 0));

describe("homeStats", () => {
  it("에이전트가 붙어 있는 상태만 '도는 중'으로 센다", () => {
    const stats = homeStats(
      [
        task({ id: 1, state: "Running" }),
        task({ id: 2, state: "Starting" }),
        task({ id: 3, state: "Finalizing" }),
        task({ id: 4, state: "Queued" }),
        task({ id: 5, state: "Created" }),
      ],
      NOON,
    );

    expect(stats.running).toBe(3);
  });

  it("검토 대기와 실행 승인 대기를 가른다 — 내가 할 행동이 다르다(ADR 0191)", () => {
    const stats = homeStats(
      [
        task({ id: 1, state: "AwaitingReview" }),
        task({ id: 2, state: "AwaitingReview", awaiting_kind: "question" }),
        task({ id: 3, state: "PendingApproval" }),
        task({ id: 4, state: "Running" }),
      ],
      NOON,
    );

    expect(stats.awaiting).toBe(2);
    expect(stats.pendingApproval).toBe(1);
  });

  it("'오늘 완료'는 자정 이후 종료된 Done만 — 어제 것은 넘어오지 않는다", () => {
    const stats = homeStats(
      [
        task({ id: 1, state: "Done", updated_at: MIDNIGHT + 60 }),
        task({ id: 2, state: "Done", updated_at: MIDNIGHT }),
        task({ id: 3, state: "Done", updated_at: MIDNIGHT - 1 }),
      ],
      NOON,
    );

    expect(stats.doneToday).toBe(2);
  });

  it("오늘 갱신됐어도 종료되지 않았으면 완료가 아니다", () => {
    const stats = homeStats(
      [
        task({ id: 1, state: "Running", updated_at: MIDNIGHT + 60 }),
        task({ id: 2, state: "Failed", updated_at: MIDNIGHT + 60 }),
        task({ id: 3, state: "Discarded", updated_at: MIDNIGHT + 60 }),
      ],
      NOON,
    );

    expect(stats.doneToday).toBe(0);
  });

  it("작업이 없으면 네 칸 모두 0 — 스트립 자리는 그대로 선다", () => {
    expect(homeStats([], NOON)).toEqual({
      running: 0,
      awaiting: 0,
      pendingApproval: 0,
      doneToday: 0,
    });
  });
});
