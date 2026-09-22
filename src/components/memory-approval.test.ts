import { describe, expect, it, vi } from "vitest";
import type { Memory } from "../lib/ipc";
import { approveMemoryWithConfirmation, type MemoryApprovalDeps } from "./memory-approval";

function candidate(
  status: Memory["status"],
): Pick<Memory, "id" | "status" | "knowledge_type" | "current_version"> {
  return { id: 42, status, knowledge_type: "decision", current_version: 3 };
}

function deps(approved: boolean, calls: string[]): MemoryApprovalDeps {
  return {
    askConfirmation: vi.fn(() => approved),
    confirmAndApprove: vi.fn(async () => {
      calls.push("atomic");
      return { version: 3, receipt_id: 7, already_approved: false };
    }),
  };
}

describe("approveMemoryWithConfirmation", () => {
  it("does not mutate memory when the user cancels", async () => {
    const calls: string[] = [];

    await expect(approveMemoryWithConfirmation(candidate("candidate"), deps(false, calls))).resolves
      .toBe(false);
    expect(calls).toEqual([]);
  });

  it("uses one version-guarded backend approval call", async () => {
    const calls: string[] = [];
    const approvalDeps = deps(true, calls);

    await expect(approveMemoryWithConfirmation(candidate("candidate"), approvalDeps)).resolves
      .toBe(true);
    expect(calls).toEqual(["atomic"]);
    expect(approvalDeps.confirmAndApprove).toHaveBeenCalledWith(42, 3);
    expect(approvalDeps.askConfirmation).toHaveBeenCalledWith(
      expect.stringContaining("검증된 후보로 승인"),
    );
    expect(approvalDeps.askConfirmation).not.toHaveBeenCalledWith(
      expect.stringContaining("다음 관련 작업에 적용"),
    );
  });

  it("uses the same atomic call for an item already pending review", async () => {
    const calls: string[] = [];

    await approveMemoryWithConfirmation(candidate("pending_review"), deps(true, calls));
    expect(calls).toEqual(["atomic"]);
  });

  it("rejects states that cannot enter human approval", async () => {
    const calls: string[] = [];

    await expect(
      approveMemoryWithConfirmation(candidate("verified"), deps(true, calls)),
    ).rejects.toThrow("승인 가능한 상태가 아닙니다");
    expect(calls).toEqual([]);
  });
});
