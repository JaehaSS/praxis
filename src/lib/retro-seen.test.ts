// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from "vitest";
import { hasUnread, loadSeenWeek, saveSeenWeek } from "./retro-seen";

describe("retro-seen", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("읽은 기록이 없으면 새 회고는 읽지 않은 것이다", () => {
    expect(hasUnread(1000, null)).toBe(true);
  });

  it("생성된 회고가 없으면 점을 켜지 않는다", () => {
    expect(hasUnread(null, null)).toBe(false);
    expect(hasUnread(null, 1000)).toBe(false);
  });

  it("같은 주를 다시 열어도 점이 켜지지 않는다", () => {
    // 지난 주를 되짚어 봤다고 신선도 점이 되살아나면 그 점은 신호가 아니게 된다.
    expect(hasUnread(1000, 1000)).toBe(false);
    expect(hasUnread(1000, 2000)).toBe(false);
  });

  it("더 최신 주가 생기면 켜진다", () => {
    expect(hasUnread(2000, 1000)).toBe(true);
  });

  it("localStorage를 왕복한다", () => {
    expect(loadSeenWeek()).toBeNull();
    saveSeenWeek(1_787_497_200);
    expect(loadSeenWeek()).toBe(1_787_497_200);
  });

  it("깨진 값은 없는 것으로 읽는다", () => {
    localStorage.setItem("praxis:retro-seen", "not-a-number");
    expect(loadSeenWeek()).toBeNull();
  });
});
