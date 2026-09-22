import { describe, expect, it } from "vitest";
import type { ContextReport, InjectedMemory } from "../../lib/ipc";
import { memoryEmptyState, memoryReceiptLabel } from "./memory-inspector";

const receipt: InjectedMemory = {
  memory_id: 7,
  version: 3,
  kind: "decision",
  content: "keep receipts immutable",
  confidence: null,
  evidence_count: 2,
  evidence_status: "valid",
  target_hash: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
  target_paths: ["CLAUDE.md"],
  renderer_version: 1,
  injected_at: 100,
  outcome: null,
  exists: true,
};

describe("memoryReceiptLabel", () => {
  it("shows the immutable version, evidence, block hash, targets, and renderer", () => {
    expect(memoryReceiptLabel(receipt)).toBe(
      "v3 · 2 evidence(valid) · sha256:abcdef012345 · CLAUDE.md · renderer v1",
    );
  });

  it("labels old usage-only rows as legacy receipts", () => {
    expect(memoryReceiptLabel({ ...receipt, version: null })).toBe("legacy receipt");
  });
});

function emptyReport(overrides: Partial<ContextReport> = {}): ContextReport {
  return {
    vendors: [],
    injected: [],
    capture_enabled: false,
    memory_count: 0,
    memory_counts: {
      scope_total: 0,
      actionable: 0,
      verified: 0,
      eligible: 0,
    },
    projection: { state: "applied", selected_count: 0 },
    ...overrides,
  } as ContextReport;
}

describe("memoryEmptyState", () => {
  it("does not blame disabled capture for a task with no relevant memory", () => {
    expect(memoryEmptyState(emptyReport())).toMatchObject({
      code: "no_scope_memory",
      title: "관련 범위 메모리 없음",
    });
  });

  it("keeps old Runner diagnostics unknown instead of guessing zero", () => {
    const report = emptyReport();
    delete (report as Partial<ContextReport>).memory_counts;
    delete (report as Partial<ContextReport>).projection;

    expect(memoryEmptyState(report)).toMatchObject({
      code: "unsupported",
      title: "상세 진단 미지원",
    });
  });

  it("identifies a task created before immutable projection receipts", () => {
    expect(memoryEmptyState(emptyReport({ projection: null }))).toMatchObject({
      code: "legacy_task",
      title: "구버전 작업",
    });
  });

  it("surfaces current review work without claiming it was the historical cause", () => {
    const state = memoryEmptyState(
      emptyReport({
        memory_counts: { scope_total: 4, actionable: 3, verified: 0, eligible: 0 },
      } as Partial<ContextReport>),
    );

    expect(state).toMatchObject({ code: "needs_review", title: "현재 검토 필요 3건" });
    expect(state?.detail).toContain("현재 기준");
  });

  it("separates verified-but-blocked memory from unreviewed candidates", () => {
    expect(
      memoryEmptyState(
        emptyReport({
          memory_counts: { scope_total: 2, actionable: 0, verified: 2, eligible: 0 },
        } as Partial<ContextReport>),
      ),
    ).toMatchObject({
      code: "verified_blocked",
      title: "현재 주입 자격 차단",
    });
  });

  it("does not invent an exclusion cause when current eligible memories exist", () => {
    const state = memoryEmptyState(
      emptyReport({
        memory_counts: { scope_total: 2, actionable: 0, verified: 2, eligible: 2 },
      } as Partial<ContextReport>),
    );

    expect(state).toMatchObject({
      code: "not_selected",
      title: "이 작업의 선택 기록 0건",
    });
    expect(state?.detail).toContain("생성 당시");
    expect(state?.detail).toContain("소급 판정할 수 없습니다");
  });

  it("surfaces an unresolved projection without replacing it with current counts", () => {
    expect(
      memoryEmptyState(
        emptyReport({
          projection: { state: "degraded", selected_count: 0 },
          memory_counts: { scope_total: 3, actionable: 3, verified: 0, eligible: 0 },
        } as Partial<ContextReport>),
      ),
    ).toMatchObject({
      code: "projection_unresolved",
      title: "메모리 투영 미완료",
    });
  });

  it("flags a selected projection whose immutable injection receipt is missing", () => {
    expect(
      memoryEmptyState(
        emptyReport({ projection: { state: "applied", selected_count: 2 } } as Partial<ContextReport>),
      ),
    ).toMatchObject({
      code: "receipt_incomplete",
      title: "주입 영수증 불일치",
    });
  });

  it("separates inactive-only scope from a genuinely empty scope", () => {
    expect(
      memoryEmptyState(
        emptyReport({
          memory_counts: { scope_total: 2, actionable: 0, verified: 0, eligible: 0 },
        } as Partial<ContextReport>),
      ),
    ).toMatchObject({
      code: "inactive_only",
      title: "현재 활성 메모리 없음",
    });
  });

  it("returns no empty state after an injection receipt exists", () => {
    expect(memoryEmptyState(emptyReport({ injected: [receipt] }))).toBeNull();
  });
});
