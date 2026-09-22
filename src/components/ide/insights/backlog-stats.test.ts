import { describe, expect, it } from "vitest";
import { backlogStats } from "./backlog-stats";
import type { DayItem } from "../today-items";

const NOW = 1_787_875_200;
const DAY = 86_400;

const item = (over: Partial<DayItem>): DayItem => ({
  id: 1,
  day: "backlog",
  title: "언젠가",
  note: null,
  status: "open",
  position: 0,
  repo: null,
  task_id: null,
  source: "manual",
  source_ref: null,
  created_at: NOW,
  updated_at: NOW,
  done_at: null,
  carried_from: null,
  ...over,
});

describe("backlogStats", () => {
  it("적체를 구간으로 나눈다", () => {
    const stats = backlogStats(
      [
        item({ id: 1, created_at: NOW - DAY * 5 }),
        item({ id: 2, created_at: NOW - DAY * 40 }),
        item({ id: 3, created_at: NOW - DAY * 120 }),
      ],
      NOW,
    );

    expect(stats.total).toBe(3);
    expect(stats.stale30).toBe(2);
    expect(stats.stale90).toBe(1);
    expect(stats.oldestDays).toBe(120);
  });

  it("경계값은 포함한다 — 30일째와 90일째가 이미 적체다", () => {
    const stats = backlogStats(
      [
        item({ id: 1, created_at: NOW - DAY * 30 }),
        item({ id: 2, created_at: NOW - DAY * 90 }),
      ],
      NOW,
    );

    expect(stats.stale30).toBe(2);
    expect(stats.stale90).toBe(1);
  });

  it("오래된 순으로 이름을 내되 다섯 건까지만", () => {
    const items = Array.from({ length: 8 }, (_, i) =>
      item({ id: i + 1, title: `항목 ${i + 1}`, created_at: NOW - DAY * (i + 1) }),
    );

    const stats = backlogStats(items, NOW);

    expect(stats.oldest).toHaveLength(5);
    expect(stats.oldest[0]).toEqual({ id: 8, title: "항목 8", days: 8 });
    expect(stats.oldest[4].id).toBe(4);
  });

  it("같은 날 담긴 항목은 id 순으로 갈린다 — 렌더마다 순서가 흔들리지 않는다", () => {
    const stats = backlogStats(
      [item({ id: 9, created_at: NOW - DAY }), item({ id: 2, created_at: NOW - DAY })],
      NOW,
    );

    expect(stats.oldest.map((i) => i.id)).toEqual([2, 9]);
  });

  it("빈 백로그는 0으로 답한다", () => {
    expect(backlogStats([], NOW)).toEqual({
      total: 0,
      stale30: 0,
      stale90: 0,
      oldestDays: 0,
      oldest: [],
    });
  });
});
