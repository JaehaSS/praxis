import { describe, expect, it } from "vitest";
import {
  deltaPct,
  fmtHour,
  fmtInt,
  fmtPct,
  fmtTokens,
  prettyModel,
  relativeDay,
} from "./format";

describe("fmtTokens", () => {
  it("경계값에서 단위가 바뀐다", () => {
    expect(fmtTokens(0)).toBe("0");
    expect(fmtTokens(999)).toBe("999");
    expect(fmtTokens(1000)).toBe("1.0K");
    expect(fmtTokens(45300)).toBe("45.3K");
    expect(fmtTokens(999999)).toBe("1000.0K");
    expect(fmtTokens(1e6)).toBe("1.0M");
    expect(fmtTokens(12_400_000)).toBe("12.4M");
  });
});

describe("fmtInt", () => {
  it("천단위를 구분한다", () => {
    expect(fmtInt(0)).toBe("0");
    expect(fmtInt(1234567)).toBe("1,234,567");
  });
});

describe("fmtHour", () => {
  it("오전/오후와 12시를 올바르게 표기한다", () => {
    expect(fmtHour(0)).toBe("오전 12시");
    expect(fmtHour(9)).toBe("오전 9시");
    expect(fmtHour(12)).toBe("오후 12시");
    expect(fmtHour(15)).toBe("오후 3시");
    expect(fmtHour(23)).toBe("오후 11시");
  });

  it("null은 대시로", () => {
    expect(fmtHour(null)).toBe("—");
  });
});

describe("prettyModel", () => {
  it("알려진 계열은 축약한다", () => {
    expect(prettyModel("claude-opus-4-8")).toBe("Opus 4.8");
    expect(prettyModel("claude-sonnet-4-6-20260101")).toBe("Sonnet 4.6");
    expect(prettyModel("claude-haiku-4-5")).toBe("Haiku 4.5");
  });

  it("마이너 버전이 없는 최신 명명도 처리한다", () => {
    expect(prettyModel("claude-opus-5")).toBe("Opus 5");
    expect(prettyModel("claude-fable-5")).toBe("Fable 5");
  });

  it("컨텍스트 변형 접미사를 남겨 같은 버전의 다른 판을 구분한다", () => {
    // 한도·비용이 달라 한 이름으로 합치면 인사이트에서 분간할 수 없다.
    expect(prettyModel("claude-opus-5[1m]")).toBe("Opus 5 · 1m");
    expect(prettyModel("opus[1m]")).toBe("opus[1m]"); // 버전 없는 별칭은 원문 유지
  });

  it("뒤따르는 날짜 스탬프를 마이너 버전으로 오인하지 않는다", () => {
    expect(prettyModel("claude-opus-5-20260101")).toBe("Opus 5");
    expect(prettyModel("claude-haiku-4-5-20251001")).toBe("Haiku 4.5");
  });

  it("미인식 모델 id는 원문을 유지한다", () => {
    expect(prettyModel("gpt-5")).toBe("gpt-5");
    expect(prettyModel("")).toBe("");
  });
});

describe("fmtPct", () => {
  it("분모가 0이면 대시", () => {
    expect(fmtPct(0, 0)).toBe("—");
    expect(fmtPct(5, 0)).toBe("—");
  });

  it("반올림한 퍼센트", () => {
    expect(fmtPct(1, 2)).toBe("50%");
    expect(fmtPct(84, 100)).toBe("84%");
    expect(fmtPct(2, 3)).toBe("67%");
  });
});

describe("relativeDay", () => {
  const now = new Date(2026, 6, 26); // 2026-07-26 로컬

  it("오늘/어제/N일 전", () => {
    expect(relativeDay("2026-07-26", now)).toBe("오늘");
    expect(relativeDay("2026-07-25", now)).toBe("어제");
    expect(relativeDay("2026-07-23", now)).toBe("3일 전");
    expect(relativeDay("2026-06-26", now)).toBe("30일 전");
  });

  it("미래이거나 형식이 다르면 원문", () => {
    expect(relativeDay("2026-07-27", now)).toBe("2026-07-27");
    expect(relativeDay("nope", now)).toBe("nope");
    expect(relativeDay("", now)).toBe("");
  });
});

describe("deltaPct", () => {
  it("직전이 0이거나 없으면 null", () => {
    expect(deltaPct(100, 0)).toBeNull();
    expect(deltaPct(100, null)).toBeNull();
    expect(deltaPct(100, undefined)).toBeNull();
  });

  it("증가/감소를 반올림해 반환한다", () => {
    expect(deltaPct(123, 100)).toBe(23);
    expect(deltaPct(97, 100)).toBe(-3);
    expect(deltaPct(100, 100)).toBe(0);
    expect(deltaPct(0, 50)).toBe(-100);
  });
});
