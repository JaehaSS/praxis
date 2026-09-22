import { describe, expect, it } from "vitest";
import { refreshMessage, type RefreshOutcome } from "./base-refresh";

describe("refreshMessage", () => {
  it("정상 두 가지는 아무것도 말하지 않는다", () => {
    expect(refreshMessage("main", { kind: "skipped" })).toBeNull();
    expect(refreshMessage("main", { kind: "already_current" })).toBeNull();
  });

  it("당겨온 커밋 수를 말한다", () => {
    expect(refreshMessage("main", { kind: "fast_forwarded", commits: 3 })).toContain("+3");
  });

  it("갈라졌으면 손대지 않았다는 것과 양쪽 수를 말한다", () => {
    const msg = refreshMessage("main", { kind: "diverged", ahead: 2, behind: 5 })!;
    expect(msg).toContain("최신화하지 않았습니다");
    expect(msg).toContain("앞 2");
    expect(msg).toContain("뒤 5");
  });

  it("실패해도 작업이 진행됐다는 것을 함께 말한다", () => {
    const msg = refreshMessage("main", { kind: "failed", reason: "타임아웃" })!;
    expect(msg).toContain("타임아웃");
    expect(msg).toContain("로컬 상태에서 분기");
  });

  it("다른 워크트리가 쥔 경우 원격에서 분기했다고 말한다", () => {
    expect(refreshMessage("dev", { kind: "branched_from_remote" })).toContain("origin/dev");
  });

  it("모든 종류가 문구를 갖는다 (새 종류를 추가하고 잊는 것을 막는다)", () => {
    const all: RefreshOutcome[] = [
      { kind: "skipped" },
      { kind: "no_upstream" },
      { kind: "already_current" },
      { kind: "fast_forwarded", commits: 1 },
      { kind: "branched_from_remote" },
      { kind: "diverged", ahead: 1, behind: 1 },
      { kind: "failed", reason: "x" },
    ];
    for (const o of all) expect(() => refreshMessage("main", o)).not.toThrow();
  });
});
