import { describe, expect, it } from "vitest";
import { contextPercent, contextShrinkWarning, contextWindowFor } from "./context-window";

describe("contextWindowFor", () => {
  it("벤더별 기본 윈도를 매핑하고 미지의 값은 200K로 폴백한다", () => {
    expect(contextWindowFor("claude")).toBe(200_000);
    expect(contextWindowFor("codex")).toBe(272_000);
    expect(contextWindowFor("gemini")).toBe(1_000_000);
    expect(contextWindowFor("unknown")).toBe(200_000);
    expect(contextWindowFor(null)).toBe(200_000);
    expect(contextWindowFor(undefined)).toBe(200_000);
  });

  it("claude `[1m]` 모델은 1M 윈도로 연다", () => {
    expect(contextWindowFor("claude", "opus[1m]")).toBe(1_000_000);
    expect(contextWindowFor("claude", "claude-opus-5[1m]")).toBe(1_000_000);
    expect(contextWindowFor("claude", "opus")).toBe(200_000);
    expect(contextWindowFor("claude", null)).toBe(200_000);
  });

  it("관측 토큰으로 티어를 자동 승격하지 않는다", () => {
    expect(contextWindowFor("claude", null)).toBe(200_000);
  });
});

describe("contextPercent", () => {
  it("사용률을 정수 퍼센트로 반올림한다", () => {
    expect(contextPercent(81_500, "claude")).toBe(41);
    expect(contextPercent(100_000, "claude")).toBe(50);
    expect(contextPercent(17_496, "codex")).toBe(6);
  });

  it("관측값이 없거나 0이면 null — 세그먼트를 숨긴다", () => {
    expect(contextPercent(null, "claude")).toBeNull();
    expect(contextPercent(0, "claude")).toBeNull();
  });

  it("`[1m]` 모델은 1M 기준으로 계산한다 — 200K 상한에 눌러앉지 않는다", () => {
    expect(contextPercent(300_000, "claude", "opus[1m]")).toBe(30);
  });

  it("모델 표기가 없으면 200K를 유지하고, 실제 관측 윈도만 별도로 쓴다", () => {
    expect(contextPercent(300_000, "claude")).toBe(100);
    expect(contextPercent(300_000, "claude", null, 1_000_000)).toBe(30);
  });

  it("최상위 윈도까지 넘는 관측(집계 오차)은 100%로 캡한다", () => {
    expect(contextPercent(1_200_000, "claude", "opus[1m]")).toBe(100);
    expect(contextPercent(300_000, "codex")).toBe(100);
  });
});

describe("contextShrinkWarning", () => {
  it("1M에서 200K로 내려가며 관측이 대상을 넘으면 경고한다 — 다음 턴이 실패할 자리다", () => {
    const warning = contextShrinkWarning("claude", "opus", "opus[1m]", 300_000);
    expect(warning).toContain((300_000).toLocaleString());
    expect(warning).toContain((200_000).toLocaleString());
  });

  it("대상 윈도가 관측을 담으면 조용하다", () => {
    expect(contextShrinkWarning("claude", "opus", "opus[1m]", 150_000)).toBeNull();
  });

  it("넓어지는 전환은 경고하지 않는다", () => {
    expect(contextShrinkWarning("claude", "opus[1m]", "opus", 300_000)).toBeNull();
  });

  it("윈도가 그대로면 경고하지 않는다 — 좁아지지 않았는데 묻는 창은 읽히지 않는다", () => {
    // 단일 티어 벤더는 어떤 전환도 윈도를 바꾸지 않는다. 현재 모델과 견주지 않으면
    // 관측이 티어를 넘는 순간부터 전환마다 경고를 맞는다.
    expect(contextShrinkWarning("codex", "gpt-5.5", "gpt-5.6-sol", 300_000)).toBeNull();
    // 커스텀 CLI는 TIERS에 없어 200K로 떨어진다. 같은 이유로 조용해야 한다.
    expect(
      contextShrinkWarning("crush", "anthropic/claude-opus-4-8", "google/gemini-3-pro", 300_000),
    ).toBeNull();
    expect(contextShrinkWarning("claude", "sonnet", "opus", 300_000)).toBeNull();
  });

  it("해제(빈 문자열)는 벤더 기본을 여기서 알 수 없으므로 경고하지 않는다", () => {
    // 설정의 `model:<agent>`가 `[1m]` 모델이면 오히려 넓어진다. 모르는 것을 좁다고 단정하지 않는다.
    expect(contextShrinkWarning("claude", "", "opus[1m]", 300_000)).toBeNull();
    expect(contextShrinkWarning("claude", "   ", "opus[1m]", 300_000)).toBeNull();
  });

  it("경계값 — 윈도와 정확히 같은 관측은 담기므로 조용하다", () => {
    expect(contextShrinkWarning("claude", "opus", "opus[1m]", 200_000)).toBeNull();
    expect(contextShrinkWarning("claude", "opus", "opus[1m]", 200_001)).not.toBeNull();
  });

  it("관측이 없거나 0 이하면 판단 근거가 없으므로 조용하다", () => {
    expect(contextShrinkWarning("claude", "opus", "opus[1m]", null)).toBeNull();
    expect(contextShrinkWarning("claude", "opus", "opus[1m]", undefined)).toBeNull();
    expect(contextShrinkWarning("claude", "opus", "opus[1m]", 0)).toBeNull();
    expect(contextShrinkWarning("claude", "opus", "opus[1m]", -1)).toBeNull();
  });

  it("현재 모델을 모르면 최하위 티어로 보아 보수적으로 침묵한다", () => {
    // current가 비면 200K로 잡히므로 200K 대상은 "좁아지지 않음"이 된다.
    expect(contextShrinkWarning("claude", "opus", null, 300_000)).toBeNull();
  });

  it("실행 윈도는 모델이 정한다", () => {
    expect(contextWindowFor("claude", null)).toBe(200_000);
    expect(contextShrinkWarning("claude", "opus", "opus[1m]", 300_000)).not.toBeNull();
  });
});
