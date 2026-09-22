import { describe, expect, it } from "vitest";
import {
  addMonth,
  fmtDayLabel,
  itemsOf,
  kstToday,
  monthBounds,
  monthGrid,
  monthOf,
  summarize,
} from "./plan-calendar";
import type { DayItem } from "../today-items";

const item = (day: string, id: number, status: DayItem["status"] = "open"): DayItem => ({
  id,
  day,
  title: `항목 ${id}`,
  note: null,
  status,
  position: id,
  repo: null,
  task_id: null,
  source: "manual",
  source_ref: null,
  created_at: 0,
  updated_at: 0,
  done_at: null,
  carried_from: null,
});

describe("kstToday", () => {
  it("UTC 15:00을 넘기면 KST로는 다음 날이다", () => {
    // 이 경계를 놓치면 저녁 시간대에 "오늘"이 하루 밀려 빈 목록이 뜬다 (DR-4).
    expect(kstToday(Date.parse("2026-08-07T14:59:59Z"))).toBe("2026-08-07");
    expect(kstToday(Date.parse("2026-08-07T15:00:00Z"))).toBe("2026-08-08");
  });

  it("자정 직후 UTC는 KST로 같은 날 오전이다", () => {
    expect(kstToday(Date.parse("2026-08-07T00:30:00Z"))).toBe("2026-08-07");
  });
});

describe("monthOf", () => {
  it("날짜에서 달을 떼어낸다", () => {
    expect(monthOf("2026-08-07")).toBe("2026-08");
  });
});

describe("addMonth", () => {
  it("연도 경계를 넘는다", () => {
    expect(addMonth("2026-12", 1)).toBe("2027-01");
    expect(addMonth("2026-01", -1)).toBe("2025-12");
  });

  it("여러 달을 건너뛴다", () => {
    expect(addMonth("2026-08", 5)).toBe("2027-01");
  });
});

describe("monthBounds", () => {
  it("달의 마지막 날을 정확히 잡는다", () => {
    expect(monthBounds("2026-08")).toEqual({ from: "2026-08-01", to: "2026-08-31" });
    expect(monthBounds("2026-09")).toEqual({ from: "2026-09-01", to: "2026-09-30" });
  });

  it("윤년 2월은 29일까지다", () => {
    expect(monthBounds("2028-02").to).toBe("2028-02-29");
    expect(monthBounds("2026-02").to).toBe("2026-02-28");
  });
});

describe("monthGrid", () => {
  it("항상 6주 × 7일이다 — 달마다 높이가 출렁이지 않게", () => {
    const grid = monthGrid("2026-08");
    expect(grid).toHaveLength(6);
    for (const week of grid) expect(week).toHaveLength(7);
  });

  it("첫 칸은 일요일이고 그 달 1일을 덮는 주에서 시작한다", () => {
    // 2026-08-01은 토요일 → 첫 주는 7/26(일)~8/1(토)
    const grid = monthGrid("2026-08");
    expect(grid[0][0].day).toBe("2026-07-26");
    expect(grid[0][6].day).toBe("2026-08-01");
  });

  it("앞뒤 달 날짜를 outside로 표시한다", () => {
    const grid = monthGrid("2026-08");
    expect(grid[0][0].outside).toBe(true);
    expect(grid[0][6].outside).toBe(false);
    expect(grid[0][6].date).toBe(1);
  });

  it("1일이 일요일인 달도 앞 주를 만들지 않는다", () => {
    // 2026-11-01은 일요일 → 격자가 그날부터 시작한다
    const grid = monthGrid("2026-11");
    expect(grid[0][0].day).toBe("2026-11-01");
    expect(grid[0][0].outside).toBe(false);
  });
});

describe("summarize", () => {
  it("날짜별로 상태를 센다", () => {
    const map = summarize([
      item("2026-08-03", 1, "done"),
      item("2026-08-03", 2, "open"),
      item("2026-08-05", 3, "dropped"),
    ]);

    expect(map.get("2026-08-03")).toMatchObject({ done: 1, open: 0 + 1, dropped: 0 });
    expect(map.get("2026-08-05")).toMatchObject({ done: 0, open: 0, dropped: 1 });
  });

  it("dropped는 진행률 분모에서 빠진다", () => {
    const map = summarize([
      item("2026-08-03", 1, "done"),
      item("2026-08-03", 2, "dropped"),
    ]);

    // 접은 계획이 달성률을 깎으면 항목을 접지 않고 방치하게 된다.
    const summary = map.get("2026-08-03")!;
    expect(summary.total).toBe(1);
    expect(summary.ratio).toBe(1);
  });

  it("항목이 없으면 빈 맵이다", () => {
    expect(summarize([]).size).toBe(0);
  });
});

describe("itemsOf", () => {
  it("그 날 항목만 순서대로 돌려준다", () => {
    const all = [item("2026-08-03", 1), item("2026-08-05", 2), item("2026-08-03", 3)];
    expect(itemsOf(all, "2026-08-03").map((i) => i.id)).toEqual([1, 3]);
  });
});

describe("fmtDayLabel", () => {
  it("앞의 0을 떼고 한국어로 읽는다", () => {
    expect(fmtDayLabel("2026-08-07")).toBe("8월 7일");
    expect(fmtDayLabel("2026-12-25")).toBe("12월 25일");
  });
});
