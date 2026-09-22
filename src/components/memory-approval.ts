import type { Memory } from "../lib/ipc";
import type { ConfirmedApproval } from "../lib/ipc";

type ApprovalMemory = Pick<Memory, "id" | "status" | "knowledge_type" | "current_version">;

export interface MemoryApprovalDeps {
  askConfirmation: (message: string) => boolean;
  confirmAndApprove: (id: number, expectedVersion: number) => Promise<ConfirmedApproval>;
}

export async function approveMemoryWithConfirmation(
  memory: ApprovalMemory,
  deps: MemoryApprovalDeps,
): Promise<boolean> {
  if (!canApprove(memory.status)) {
    throw new Error("승인 가능한 상태가 아닙니다");
  }
  const confirmed = deps.askConfirmation(
    `${memory.knowledge_type} 메모리 #${memory.id}의 본문과 근거를 직접 확인했고 검증된 후보로 승인할까요?`,
  );
  if (!confirmed) return false;
  await deps.confirmAndApprove(memory.id, memory.current_version);
  return true;
}

function canApprove(status: Memory["status"]): boolean {
  return (
    status === "candidate" ||
    status === "pending_review" ||
    status === "stale" ||
    status === "legacy_unverified"
  );
}
