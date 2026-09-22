import type { MemoryEvidence } from "../lib/ipc";

export type EvidenceLoadState =
  | { memoryId: number; status: "loading" }
  | { memoryId: number; status: "ready"; rows: MemoryEvidence[] }
  | { memoryId: number; status: "error"; message: string };

export interface EvidenceActionError {
  memoryId: number;
  message: string;
}

export function evidenceActionErrorForMemory(
  error: EvidenceActionError | null,
  memoryId: number,
): string | null {
  if (error?.memoryId !== memoryId) return null;
  return error.message;
}

export function evidenceStateForMemory(
  state: EvidenceLoadState,
  memoryId: number,
): EvidenceLoadState {
  if (state.memoryId === memoryId) return state;
  return { memoryId, status: "loading" };
}

export function evidenceCountLabel(state: EvidenceLoadState): string {
  if (state.status === "ready") return `${state.rows.length}건`;
  if (state.status === "error") return "확인 실패";
  return "확인 중";
}

export function evidenceRequestBelongsToMemory(
  activeMemoryId: number | null,
  requestMemoryId: number,
): boolean {
  return activeMemoryId === requestMemoryId;
}

export function evidenceLocatorLabel(evidence: MemoryEvidence): string {
  try {
    const locator = JSON.parse(evidence.locator_json) as Record<string, unknown>;
    if (typeof locator.relative_path === "string") return locator.relative_path;
    if (typeof locator.url === "string") return locator.url;
    if (locator.actor_kind === "human") return "사람 확인";
  } catch {
    return "locator 손상";
  }
  return "backend receipt";
}

export function evidenceStatusTone(status: MemoryEvidence["status"]): string {
  if (status === "valid") return "text-status-done";
  if (status === "unknown") return "text-status-awaiting";
  return "text-status-failed";
}

export function evidenceStatusLabel(status: MemoryEvidence["status"]): string {
  const labels: Record<MemoryEvidence["status"], string> = {
    valid: "유효",
    changed: "원본 변경",
    missing: "원본 없음",
    expired: "만료",
    unknown: "확인 불가",
  };
  return labels[status];
}

export function evidenceTimestampLabel(value: number | null): string {
  if (value === null) return "미검사";
  return new Date(value * 1000).toISOString();
}
