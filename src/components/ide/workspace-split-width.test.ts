import { describe, expect, it } from "vitest";
import {
  MIN_CODE_WIDTH,
  MIN_SESSION_WIDTH,
  SPLIT_ENTER,
  SPLIT_EXIT,
  clampSplit,
  nextSplitMode,
  readSplit,
  writeSplit,
} from "./workspace-split-width";

const storage = (initial?: string) => {
  const map = new Map<string, string>();
  if (initial != null) map.set("praxis-workspace-split", initial);
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
  };
};

describe("임계 상수", () => {
  it("2열 최소 요구는 두 열의 최소 폭 합이다", () => {
    expect(SPLIT_EXIT).toBe(MIN_CODE_WIDTH + MIN_SESSION_WIDTH);
  });

  it("복귀 임계가 진입 임계보다 높다 — 경계에서 모드가 깜빡이지 않는다", () => {
    expect(SPLIT_ENTER).toBeGreaterThan(SPLIT_EXIT);
  });
});

describe("nextSplitMode — 히스테리시스", () => {
  it("2열에서 폭이 부족해지면 탭으로 폴백한다", () => {
    expect(nextSplitMode("split", SPLIT_EXIT - 1)).toBe("tabs");
  });

  it("2열에서 최소 폭을 지키면 유지한다", () => {
    expect(nextSplitMode("split", SPLIT_EXIT)).toBe("split");
  });

  it("탭에서 복귀 임계 미만이면 탭을 유지한다 — 되돌아가지 않는다", () => {
    expect(nextSplitMode("tabs", SPLIT_ENTER - 1)).toBe("tabs");
  });

  it("탭에서 복귀 임계 이상이면 2열로 돌아간다", () => {
    expect(nextSplitMode("tabs", SPLIT_ENTER)).toBe("split");
  });

  // 두 임계 사이는 "직전 모드를 그대로 둔다"가 히스테리시스의 본질이다.
  it("두 임계 사이 구간은 직전 모드를 보존한다", () => {
    const between = Math.floor((SPLIT_EXIT + SPLIT_ENTER) / 2);
    expect(nextSplitMode("split", between)).toBe("split");
    expect(nextSplitMode("tabs", between)).toBe("tabs");
  });
});

describe("clampSplit — 분할 비율", () => {
  it("세션 열이 최소 폭 아래로 내려가지 않는다", () => {
    expect(clampSplit(100, 1000)).toBe(MIN_SESSION_WIDTH);
  });

  it("코드 열이 최소 폭 아래로 내려가지 않는다", () => {
    expect(clampSplit(900, 1000)).toBe(1000 - MIN_CODE_WIDTH);
  });

  it("두 최소 폭 사이면 그대로 둔다", () => {
    expect(clampSplit(500, 1400)).toBe(500);
  });
});

describe("영속 — 세션 열 폭", () => {
  it("저장된 값이 없으면 기본 폭을 쓴다", () => {
    expect(readSplit(storage()).width).toBeGreaterThanOrEqual(MIN_SESSION_WIDTH);
  });

  it("저장한 폭을 되읽는다", () => {
    const s = storage();
    writeSplit(s, { width: 520 });
    expect(readSplit(s)).toEqual({ width: 520 });
  });

  it("깨진 값은 기본 폭으로 degrade한다 — 파싱 실패가 워크스페이스를 막지 않는다", () => {
    expect(readSplit(storage("not json")).width).toBeGreaterThanOrEqual(MIN_SESSION_WIDTH);
  });
});
