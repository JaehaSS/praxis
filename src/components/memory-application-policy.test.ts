import { describe, expect, it } from "vitest";
import type { Memory } from "../lib/ipc";
import {
  confirmMessage,
  designatability,
  groupPreview,
  policyOf,
} from "./memory-application-policy";

const base: Memory = {
  id: 1,
  tier: "project",
  scope_key: "/repo",
  kind: "decision",
  content: "락파일은 직접 수정하지 않는다",
  source_session: null,
  confidence: 0.5,
  usage_count: 0,
  last_used: null,
  created_at: 0,
  knowledge_type: "decision",
  status: "verified",
  current_version: 1,
  utility_score: 0.5,
  review_due_at: null,
  verified_at: null,
  stale_at: null,
  archived_at: null,
  application_policy: "relevance",
  dormant: false,
};

const memory = (patch: Partial<Memory>): Memory => ({ ...base, ...patch });

describe("designatability", () => {
  it("allows a verified project decision or convention", () => {
    expect(designatability(base)).toEqual({ kind: "designatable" });
    expect(designatability(memory({ knowledge_type: "convention" }))).toEqual({
      kind: "designatable",
    });
  });

  it("blocks global tier, non-rule types, and unverified rows with a reason", () => {
    for (const patch of [
      { tier: "global" as const },
      { knowledge_type: "claim" as const },
      { status: "candidate" as const },
    ]) {
      const result = designatability(memory(patch));
      expect(result.kind).toBe("blocked");
      // 왜 안 되는지 말하지 않으면 사용자는 버튼이 없는 이유를 추측해야 한다.
      expect(result.kind === "blocked" && result.reason.length).toBeGreaterThan(0);
    }
  });

  it("always offers release for a designated rule, even when it went stale", () => {
    // stale 규칙은 작업 시작을 막는다 — 해제까지 막으면 사용자가 갇힌다.
    const stale = memory({ status: "stale", application_policy: "must_apply" });
    expect(designatability(stale)).toEqual({ kind: "designated" });
  });
});

describe("policyOf", () => {
  it("returns null when the response predates the field", () => {
    const legacy = memory({});
    delete (legacy as { application_policy?: unknown }).application_policy;
    expect(policyOf(legacy)).toBeNull();
  });
});

describe("confirmMessage", () => {
  it("says what changes in each direction", () => {
    expect(confirmMessage(base)).toContain("항상 적용할까요");
    expect(confirmMessage(memory({ application_policy: "must_apply" }))).toContain("해제할까요");
  });
});

describe("groupPreview", () => {
  it("splits the two sections without reordering", () => {
    const hits = [
      memory({ id: 9, application_policy: "must_apply", content: "규칙" }),
      memory({ id: 3, content: "관련 A" }),
      memory({ id: 4, content: "관련 B" }),
    ];
    const groups = groupPreview(hits);
    expect(groups.mustApply.map((m) => m.id)).toEqual([9]);
    // 서버가 정한 순서를 그대로 둔다 — 재정렬하면 표시와 실제 투영이 갈린다.
    expect(groups.relevant.map((m) => m.id)).toEqual([3, 4]);
  });

  it("treats a field-less legacy response as relevant", () => {
    const legacy = memory({ id: 5 });
    delete (legacy as { application_policy?: unknown }).application_policy;
    expect(groupPreview([legacy]).mustApply).toEqual([]);
    expect(groupPreview([legacy]).relevant.map((m) => m.id)).toEqual([5]);
  });
});
