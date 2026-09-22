import { describe, expect, it } from "vitest";
import { isDirectRun, preservedBranchLabel } from "./discard-confirm";

const isolated = {
  repo: "/w/praxis",
  branch: "praxis/fix-login-1724",
  worktree_path: "/w/praxis/.praxis/worktrees/praxis-fix-login-1724",
};
const direct = { repo: "/w/praxis", branch: "dev", worktree_path: "/w/praxis" };

describe("isDirectRun", () => {
  it("직접 실행 판별은 경로 동일성이다 (백엔드 is_direct와 같은 규칙)", () => {
    expect(isDirectRun(direct)).toBe(true);
    expect(isDirectRun(isolated)).toBe(false);
  });
});

describe("preservedBranchLabel", () => {
  it("폐기된 격리 작업은 보존 브랜치를 이름으로 남긴다", () => {
    expect(preservedBranchLabel({ ...isolated, state: "Discarded" })).toBe(
      `복구 확인: ${isolated.worktree_path} · 작업 브랜치 ${isolated.branch}`,
    );
  });

  it("아직 폐기되지 않은 작업에는 붙이지 않는다", () => {
    expect(preservedBranchLabel({ ...isolated, state: "AwaitingReview" })).toBeNull();
  });

  it("직접 실행은 남긴 브랜치가 없다", () => {
    expect(preservedBranchLabel({ ...direct, state: "Discarded" })).toBeNull();
  });
});
