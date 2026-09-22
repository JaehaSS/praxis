import { describe, expect, it } from "vitest";
import type { Memory } from "../lib/ipc";
import { memoryReadiness } from "./memory-readiness";

function memory(overrides: Partial<Memory> = {}): Memory {
  return {
    id: 1,
    tier: "project",
    scope_key: "/repo",
    kind: "decision",
    content: "Keep the API stable",
    source_session: null,
    confidence: 0,
    usage_count: 0,
    last_used: null,
    created_at: 1,
    knowledge_type: "decision",
    status: "candidate",
    current_version: 1,
    utility_score: 0,
    review_due_at: null,
    verified_at: null,
    stale_at: null,
    archived_at: null,
    dormant: false,
    ...overrides,
  };
}

describe("memoryReadiness", () => {
  it("keeps an older Runner's missing summary unknown instead of treating it as zero", () => {
    expect(memoryReadiness(memory())).toEqual({
      tone: "unknown",
      label: "근거 요약 미지원",
      detail: "근거 패널을 열어 현재 버전을 확인하세요.",
      canApprove: true,
    });
  });

  it("explains that explicit approval can create the first human evidence", () => {
    expect(
      memoryReadiness(memory({ evidence_count: 0, blocking_evidence_count: 0 })),
    ).toEqual({
      tone: "needs_action",
      label: "직접 확인 필요",
      detail: "현재 버전 근거 0건 · 승인 확인이 사람 확인 근거를 만듭니다.",
      canApprove: true,
    });
  });

  it("blocks approval when any current-version evidence is invalid or expired", () => {
    const readiness = memoryReadiness(
      memory({ evidence_count: 3, blocking_evidence_count: 1 }),
    );

    expect(readiness.tone).toBe("blocked");
    expect(readiness.label).toBe("근거 차단 1건");
    expect(readiness.detail).toContain("주입되지 않습니다");
    expect(readiness.canApprove).toBe(false);
  });

  it("calls verified memory an eligible candidate rather than an injection", () => {
    const readiness = memoryReadiness(
      memory({
        status: "verified",
        evidence_count: 2,
        blocking_evidence_count: 0,
      }),
    );

    expect(readiness.tone).toBe("ready");
    expect(readiness.label).toBe("주입 후보");
    expect(readiness.detail).toContain("유효 근거 2건");
    expect(readiness.detail).toContain("선택될 때만");
    expect(readiness.canApprove).toBe(false);
  });

  it("blocks a verified row whose current version has no evidence", () => {
    expect(
      memoryReadiness(
        memory({
          status: "verified",
          evidence_count: 0,
          blocking_evidence_count: 0,
        }),
      ),
    ).toMatchObject({
      tone: "blocked",
      label: "현재 버전 근거 없음",
      canApprove: false,
    });
  });

  it.each([
    ["pending_review", "승인 대기"],
    ["stale", "재검토 필요"],
    ["legacy_unverified", "이관 검토"],
    ["candidate", "검토 준비"],
  ] as const)("gives %s its own review-stage label", (status, label) => {
    expect(
      memoryReadiness(
        memory({
          status,
          evidence_count: 1,
          blocking_evidence_count: 0,
        }),
      ).label,
    ).toBe(label);
  });

  it.each([
    ["archived", "보관됨"],
    ["rejected", "거부됨"],
  ] as const)("keeps %s memory inactive", (status, label) => {
    expect(
      memoryReadiness(
        memory({
          status,
          evidence_count: 1,
          blocking_evidence_count: 0,
        }),
      ),
    ).toMatchObject({
      tone: "inactive",
      label,
      canApprove: false,
    });
  });

  it("keeps stale memory reviewable but dormant memory out of activation", () => {
    expect(
      memoryReadiness(
        memory({ status: "stale", evidence_count: 1, blocking_evidence_count: 0 }),
      ).canApprove,
    ).toBe(true);
    expect(
      memoryReadiness(
        memory({ dormant: true, evidence_count: 0, blocking_evidence_count: 0 }),
      ),
    ).toMatchObject({
      tone: "inactive",
      label: "휴면",
      canApprove: false,
    });
  });
});
