import { describe, expect, it } from "vitest";
import {
  carriedLabel,
  groupSuggestions,
  moveItem,
  progress,
  repoBadge,
  showRepoBadges,
  sortItems,
  staleDays,
  type DayItem,
} from "./today-items";

const item = (over: Partial<DayItem>): DayItem => ({
  id: 1,
  day: "2026-08-03",
  title: "t",
  note: null,
  status: "open",
  position: 0,
  repo: null,
  task_id: null,
  source: "manual",
  source_ref: null,
  created_at: 0,
  updated_at: 0,
  done_at: null,
  carried_from: null,
  ...over,
});

describe("progress", () => {
  it("dropped는 분모에서 뺀다 — 안 하기로 한 것이 진행률을 깎으면 안 된다", () => {
    const items = [
      item({ id: 1, status: "done" }),
      item({ id: 2, status: "open" }),
      item({ id: 3, status: "dropped" }),
    ];
    expect(progress(items)).toEqual({ done: 1, total: 2, ratio: 0.5 });
  });

  it("항목이 없으면 ratio는 0이고 NaN이 아니다", () => {
    expect(progress([])).toEqual({ done: 0, total: 0, ratio: 0 });
  });

  it("전부 dropped여도 NaN이 아니다", () => {
    expect(progress([item({ status: "dropped" })])).toEqual({ done: 0, total: 0, ratio: 0 });
  });
});

describe("sortItems", () => {
  it("position 오름차순, 같으면 id 오름차순 (안정 정렬)", () => {
    const items = [
      item({ id: 3, position: 1 }),
      item({ id: 1, position: 0 }),
      item({ id: 2, position: 1 }),
    ];
    expect(sortItems(items).map((i) => i.id)).toEqual([1, 2, 3]);
  });

  it("원본 배열을 변형하지 않는다", () => {
    const items = [item({ id: 2, position: 1 }), item({ id: 1, position: 0 })];
    sortItems(items);
    expect(items.map((i) => i.id)).toEqual([2, 1]);
  });
});

describe("moveItem", () => {
  it("위로 한 칸 옮긴 id 순서를 돌려준다", () => {
    const items = [item({ id: 1, position: 0 }), item({ id: 2, position: 1 })];
    expect(moveItem(items, 2, -1)).toEqual([2, 1]);
  });

  it("경계 밖으로는 움직이지 않는다", () => {
    const items = [item({ id: 1, position: 0 }), item({ id: 2, position: 1 })];
    expect(moveItem(items, 1, -1)).toEqual([1, 2]);
    expect(moveItem(items, 2, 1)).toEqual([1, 2]);
  });

  it("없는 id는 원래 순서를 돌려준다", () => {
    const items = [item({ id: 1, position: 0 }), item({ id: 2, position: 1 })];
    expect(moveItem(items, 99, -1)).toEqual([1, 2]);
  });
});

describe("repoBadge", () => {
  it("경로의 마지막 세그먼트만 남긴다", () => {
    expect(repoBadge("/Users/me/work/praxis")).toBe("praxis");
    expect(repoBadge("/Users/me/work/praxis/")).toBe("praxis");
  });
});

describe("showRepoBadges", () => {
  it("한 레포뿐이고 그게 지금 보는 레포면 붙이지 않는다", () => {
    const items = [item({ id: 1, repo: "/a" }), item({ id: 2, repo: "/a" })];
    expect(showRepoBadges(items, "/a")).toBe(false);
  });

  it("여러 레포가 섞이면 붙인다", () => {
    const items = [item({ id: 1, repo: "/a" }), item({ id: 2, repo: "/b" })];
    expect(showRepoBadges(items, "/a")).toBe(true);
  });

  it("다른 레포의 일만 있으면 붙인다 — 지금 보는 곳의 일이 아님을 알아야 한다", () => {
    expect(showRepoBadges([item({ id: 1, repo: "/b" })], "/a")).toBe(true);
    expect(showRepoBadges([item({ id: 1, repo: "/b" })], undefined)).toBe(true);
  });

  it("레포가 없는 항목뿐이면 붙일 것이 없다", () => {
    expect(showRepoBadges([item({ id: 1, repo: null })], "/a")).toBe(false);
  });
});

describe("groupSuggestions", () => {
  it("출처별로 묶고 awaiting → github → memory 순서로 낸다", () => {
    const groups = groupSuggestions([
      { title: "이슈", source: "github", source_ref: "1", repo: null },
      { title: "검토 대기", source: "awaiting", source_ref: "2", repo: null },
      { title: "또 검토 대기", source: "awaiting", source_ref: "3", repo: null },
    ]);
    expect(groups.map((g) => g.source)).toEqual(["awaiting", "github"]);
    expect(groups[0].items).toHaveLength(2);
    expect(groups[0].label).toBe("검토 대기가 오래된 작업");
  });

  it("빈 입력은 빈 그룹", () => {
    expect(groupSuggestions([])).toEqual([]);
  });
});

describe("carriedLabel", () => {
  it("오늘 정한 항목은 표시가 없다", () => {
    expect(carriedLabel(item({ carried_from: null }))).toBeNull();
  });

  it("하루 전에서 왔으면 '어제'", () => {
    expect(carriedLabel(item({ day: "2026-08-03", carried_from: "2026-08-02" }))).toBe("어제");
  });

  // 주말·휴가로 앱을 안 열면 `carried_from`이 하루 전이 아니다.
  it("이틀 넘게 건너뛰었으면 날짜를 그대로 보여준다", () => {
    expect(carriedLabel(item({ day: "2026-08-03", carried_from: "2026-07-31" }))).toBe("7/31");
  });

  it("월 경계를 넘어도 '어제'를 놓치지 않는다", () => {
    expect(carriedLabel(item({ day: "2026-08-01", carried_from: "2026-07-31" }))).toBe("어제");
  });
});

describe("staleDays", () => {
  const now = 1_800_000_000;
  const at = (created_at: number) => ({ created_at });

  it("생성 시각에서 날수를 센다", () => {
    expect(staleDays(at(now - 86400 * 40), now)).toBe(40);
    expect(staleDays(at(now), now)).toBe(0);
  });

  it("하루가 덜 지났으면 0이다 — 반올림하지 않는다", () => {
    expect(staleDays(at(now - 86400 + 1), now)).toBe(0);
  });

  it("미래에 만들어진 항목도 음수가 되지 않는다", () => {
    expect(staleDays(at(now + 86400), now)).toBe(0);
  });
});
