import { describe, expect, it } from "vitest";
import {
  dedupeTargets,
  isSelfTarget,
  resolveOutcome,
  shouldFallbackToReferences,
  targetLabel,
} from "./lsp";
import type { LspTarget } from "./ipc";

const target = (over: Partial<LspTarget> = {}): LspTarget => ({
  path: "src/main.rs",
  abs_path: "/w/src/main.rs",
  line: 10,
  column: 5,
  external: false,
  ...over,
});

describe("dedupeTargets", () => {
  it("같은 파일·줄·열의 중복을 하나로 접는다", () => {
    expect(dedupeTargets([target(), target(), target({ line: 11 })])).toHaveLength(2);
  });

  it("줄이 같아도 열이 다르면 별개로 둔다", () => {
    expect(dedupeTargets([target({ column: 5 }), target({ column: 9 })])).toHaveLength(2);
  });
});

describe("isSelfTarget", () => {
  it("커서가 선 줄을 가리키면 제자리다", () => {
    expect(isSelfTarget(target({ line: 10 }), { path: "src/main.rs", line: 10 })).toBe(true);
  });

  it("파일이 다르면 제자리가 아니다", () => {
    expect(isSelfTarget(target({ path: "src/lib.rs" }), { path: "src/main.rs", line: 10 })).toBe(
      false,
    );
  });

  it("worktree 밖 결과는 제자리가 될 수 없다", () => {
    // external은 path가 null이라 경로 비교가 성립하지 않는다.
    expect(
      isSelfTarget(target({ path: null, external: true }), { path: "src/main.rs", line: 10 }),
    ).toBe(false);
  });
});

describe("shouldFallbackToReferences", () => {
  it("정의 결과가 제자리뿐이면 사용처로 넘어간다", () => {
    const at = { path: "src/main.rs", line: 10 };
    expect(shouldFallbackToReferences([target()], at)).toBe(true);
  });

  it("다른 곳을 하나라도 가리키면 그냥 점프한다", () => {
    const at = { path: "src/main.rs", line: 10 };
    expect(shouldFallbackToReferences([target(), target({ line: 42 })], at)).toBe(false);
  });

  it("결과가 없으면 폴백하지 않는다 — 없는 건 없는 것", () => {
    expect(shouldFallbackToReferences([], { path: "src/main.rs", line: 10 })).toBe(false);
  });
});

describe("resolveOutcome", () => {
  it("빈 결과는 none", () => {
    expect(resolveOutcome([])).toEqual({ kind: "none" });
  });

  it("하나면 바로 점프", () => {
    expect(resolveOutcome([target()])).toEqual({ kind: "jump", target: target() });
  });

  it("여럿이면 고르게 한다", () => {
    const out = resolveOutcome([target(), target({ line: 20 })]);
    expect(out.kind).toBe("choose");
  });

  it("중복을 접은 뒤 하나만 남으면 고르지 않고 점프한다", () => {
    expect(resolveOutcome([target(), target()]).kind).toBe("jump");
  });
});

describe("targetLabel", () => {
  it("worktree 안이면 상대 경로", () => {
    expect(targetLabel(target())).toBe("src/main.rs:10");
  });

  it("worktree 밖이면 절대 경로", () => {
    expect(
      targetLabel(target({ path: null, abs_path: "/usr/lib/core.rs", external: true, line: 3 })),
    ).toBe("/usr/lib/core.rs:3");
  });
});
