import { describe, expect, it } from "vitest";
import { stageLabel, ensembleLabel, elapsedLabel } from "./creation-stage";

describe("stageLabel", () => {
  it("null이면 '세션 준비 중'", () => {
    expect(stageLabel(null)).toBe("세션 준비 중");
  });

  it("refresh는 base 브랜치 이름을 싣는다", () => {
    expect(stageLabel("refresh", "dev")).toBe("base 최신화(origin/dev)");
  });

  it("refresh인데 baseBranch가 없으면 'base'로 대체한다", () => {
    expect(stageLabel("refresh")).toBe("base 최신화(origin/base)");
  });

  it.each([
    ["worktree", "워크트리 생성"],
    ["bootstrap", "환경 부트스트랩"],
    ["memory", "메모리 투영"],
    ["spawn", "에이전트 기동"],
  ] as const)("%s → %s", (stage, label) => {
    expect(stageLabel(stage)).toBe(label);
  });
});

describe("ensembleLabel", () => {
  it("후보 수를 싣는다", () => {
    expect(ensembleLabel(3)).toBe("세션 준비 중 · 후보 3개");
  });
});

describe("elapsedLabel", () => {
  it("밀리초를 소수 한 자리 초로 반올림한다", () => {
    expect(elapsedLabel(1234)).toBe("1.2s");
  });

  it("음수는 0으로 바닥을 둔다", () => {
    expect(elapsedLabel(-50)).toBe("0.0s");
  });
});
