import type { ContextReport, InjectedMemory, MemoryContextCounts } from "../../lib/ipc";

export type MemoryEmptyStateCode =
  | "unsupported"
  | "legacy_task"
  | "projection_unresolved"
  | "receipt_incomplete"
  | "no_scope_memory"
  | "needs_review"
  | "verified_blocked"
  | "not_selected"
  | "inactive_only";

export interface MemoryEmptyState {
  code: MemoryEmptyStateCode;
  title: string;
  detail: string;
}

export function memoryReceiptLabel(memory: InjectedMemory): string {
  if (memory.version === null) return "legacy receipt";
  const evidenceStatus = memory.evidence_status ? `(${memory.evidence_status})` : "";
  const evidence = `${memory.evidence_count} evidence${evidenceStatus}`;
  const hash = memory.target_hash ? `sha256:${memory.target_hash.slice(0, 12)}` : "hash unavailable";
  const targets = memory.target_paths.length > 0 ? memory.target_paths.join(", ") : "target unavailable";
  const renderer = memory.renderer_version === null ? "renderer unknown" : `renderer v${memory.renderer_version}`;
  return `v${memory.version} · ${evidence} · ${hash} · ${targets} · ${renderer}`;
}

export function memoryEmptyState(report: ContextReport): MemoryEmptyState | null {
  if (report.injected.length > 0) return null;
  if (report.memory_counts === undefined || report.projection === undefined) {
    return state(
      "unsupported",
      "상세 진단 미지원",
      "이 Runner는 task별 projection·현재 scope 진단을 제공하지 않습니다.",
    );
  }
  if (report.projection === null) {
    return state(
      "legacy_task",
      "구버전 작업",
      "immutable projection receipt 도입 전에 생성되어 당시 선택 이유를 확인할 수 없습니다.",
    );
  }
  if (!["applied", "retired"].includes(report.projection.state)) {
    return state(
      "projection_unresolved",
      "메모리 투영 미완료",
      `immutable projection 상태가 ${report.projection.state}입니다. 작업 상태와 복구 기록을 확인하세요.`,
    );
  }
  if (report.projection.selected_count > 0) {
    return state(
      "receipt_incomplete",
      "주입 영수증 불일치",
      `projection은 ${report.projection.selected_count}건을 선택했지만 injection receipt를 읽지 못했습니다.`,
    );
  }
  return currentScopeState(report.memory_counts);
}

function currentScopeState(counts: MemoryContextCounts): MemoryEmptyState {
  if (counts.scope_total === 0) {
    return state(
      "no_scope_memory",
      "관련 범위 메모리 없음",
      "현재 project·global scope에 저장된 메모리가 없습니다.",
    );
  }
  if (counts.eligible > 0) {
    return state(
      "not_selected",
      "이 작업의 선택 기록 0건",
      `현재 기준 주입 후보 ${counts.eligible}건이 있지만 생성 당시의 relevance 또는 이후 상태 변경은 소급 판정할 수 없습니다.`,
    );
  }
  if (counts.verified > 0) {
    return state(
      "verified_blocked",
      "현재 주입 자격 차단",
      `verified ${counts.verified}건이 현재 evidence freshness 또는 휴면 조건을 통과하지 못합니다.`,
    );
  }
  if (counts.actionable > 0) {
    return state(
      "needs_review",
      `현재 검토 필요 ${counts.actionable}건`,
      "현재 기준 candidate·review·stale 항목이 있어 본문과 근거의 사람 확인이 필요합니다.",
    );
  }
  return state(
    "inactive_only",
    "현재 활성 메모리 없음",
    "관련 범위에는 보관·거부·휴면 등 현재 검토·주입 대상이 아닌 항목만 있습니다.",
  );
}

function state(code: MemoryEmptyStateCode, title: string, detail: string): MemoryEmptyState {
  return { code, title, detail };
}
