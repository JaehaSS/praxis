import type { Memory } from "../lib/ipc";

export type MemoryReadinessTone = "ready" | "needs_action" | "blocked" | "inactive" | "unknown";

export interface MemoryReadiness {
  tone: MemoryReadinessTone;
  label: string;
  detail: string;
  canApprove: boolean;
}

const APPROVABLE = new Set<Memory["status"]>([
  "candidate",
  "pending_review",
  "stale",
  "legacy_unverified",
]);

export function memoryReadiness(memory: Memory): MemoryReadiness {
  const inactive = inactiveReadiness(memory);
  if (inactive) return inactive;
  const canApprove = APPROVABLE.has(memory.status);
  if (memory.evidence_count === undefined || memory.blocking_evidence_count === undefined) {
    return {
      tone: "unknown",
      label: "근거 요약 미지원",
      detail: "근거 패널을 열어 현재 버전을 확인하세요.",
      canApprove,
    };
  }
  if (memory.blocking_evidence_count > 0) {
    return {
      tone: "blocked",
      label: `근거 차단 ${memory.blocking_evidence_count}건`,
      detail:
        "변경·만료·확인 불가 근거가 있어 현재 상태로는 승인·주입되지 않습니다. 재검증 후 필요하면 새 버전으로 편집하세요.",
      canApprove: false,
    };
  }
  if (memory.status === "verified") return verifiedReadiness(memory.evidence_count);
  return reviewReadiness(memory.status, memory.evidence_count);
}

function inactiveReadiness(memory: Memory): MemoryReadiness | null {
  if (memory.status === "archived" || memory.status === "rejected") {
    return {
      tone: "inactive",
      label: memory.status === "archived" ? "보관됨" : "거부됨",
      detail: "현재 검토·주입 대상이 아닙니다.",
      canApprove: false,
    };
  }
  if (!memory.dormant) return null;
  return {
    tone: "inactive",
    label: "휴면",
    detail: "미사용 30일 경과로 검색 후보에서 제외됩니다. 다시 쓸 내용이면 새 메모리로 기록하세요.",
    canApprove: false,
  };
}

function verifiedReadiness(evidenceCount: number): MemoryReadiness {
  if (evidenceCount === 0) {
    return {
      tone: "blocked",
      label: "현재 버전 근거 없음",
      detail: "승인 상태여도 유효 근거가 없으면 주입되지 않습니다.",
      canApprove: false,
    };
  }
  return {
    tone: "ready",
    label: "주입 후보",
    detail: `유효 근거 ${evidenceCount}건 · 관련 작업에서 freshness·relevance로 선택될 때만 적용됩니다.`,
    canApprove: false,
  };
}

function reviewReadiness(status: Memory["status"], evidenceCount: number): MemoryReadiness {
  if (evidenceCount === 0) {
    return {
      tone: "needs_action",
      label: "직접 확인 필요",
      detail: "현재 버전 근거 0건 · 승인 확인이 사람 확인 근거를 만듭니다.",
      canApprove: true,
    };
  }
  const label =
    status === "stale"
      ? "재검토 필요"
      : status === "pending_review"
        ? "승인 대기"
        : status === "legacy_unverified"
          ? "이관 검토"
          : "검토 준비";
  return {
    tone: "needs_action",
    label,
    detail: `현재 버전 유효 근거 ${evidenceCount}건 · 본문과 근거를 직접 확인하세요.`,
    canApprove: true,
  };
}
