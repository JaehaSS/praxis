import { describe, expect, it } from "vitest";
import {
  choiceFromOverride,
  isolationChipState,
  isolationForced,
  isolationOptions,
  overrideFromChoice,
} from "./new-task-isolation";

describe("isolationForced", () => {
  it("forces isolation for remote and ensemble tasks", () => {
    expect(isolationForced("remote", 1)).toBe(true);
    expect(isolationForced("local", 2)).toBe(true);
  });

  it("leaves a local single-agent task to the setting", () => {
    expect(isolationForced("local", 1)).toBe(false);
  });
});

describe("isolationChipState", () => {
  it("follows the global default when no override is set", () => {
    expect(isolationChipState(null, true)).toEqual({ on: true, pinned: false });
    expect(isolationChipState(null, false)).toEqual({ on: false, pinned: false });
  });

  it("lets a project override win over the global default", () => {
    expect(isolationChipState(true, false)).toEqual({ on: true, pinned: true });
    expect(isolationChipState(false, true)).toEqual({ on: false, pinned: true });
  });

  it("does not advertise isolation before the setting loads", () => {
    expect(isolationChipState(null, null)).toBe(null);
  });

  it("still reports an override that loaded before the global default", () => {
    expect(isolationChipState(true, null)).toEqual({ on: true, pinned: true });
  });
});

describe("격리 선택지", () => {
  it("override와 choice가 왕복해도 값이 보존된다", () => {
    for (const o of [null, true, false] as const)
      expect(overrideFromChoice(choiceFromOverride(o))).toBe(o);
  });

  it("격리하는 선택지는 전부 삭제를 말한다", () => {
    // 이 계약이 깨지면 사용자는 워크트리가 지워진 뒤에 그 사실을 알게 된다.
    for (const opt of isolationOptions(true))
      if (opt.label.includes("워크트리")) expect(opt.caption).toContain("삭제");
  });

  it("직접 실행 선택지는 지울 것이 없다고 말한다", () => {
    const direct = isolationOptions(true).find((o) => o.choice === "pinned-off")!;
    expect(direct.caption).toContain("지울 것도 없습니다");
    expect(direct.caption).not.toContain("삭제");
  });

  it("직접 실행 선택지는 고른 브랜치의 메인 체크아웃을 쓴다고 말한다", () => {
    const direct = isolationOptions(false).filter((o) => o.label.includes("직접 실행"));
    expect(direct).toHaveLength(2);
    for (const option of direct) expect(option.caption).toContain("선택한 브랜치");
  });

  it("'기본을 따름' 라벨이 전역 기본값을 반영한다", () => {
    expect(isolationOptions(true)[0].label).toBe("워크트리 격리");
    expect(isolationOptions(false)[0].label).toBe("직접 실행");
  });
});
