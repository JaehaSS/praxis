import { describe, expect, it } from "vitest";
import type { Memory } from "../lib/ipc";
import {
  countMemoryReviewFilters,
  filterMemoryReview,
  summarizeMemoryActivation,
} from "./memory-review";

function memory(
  id: number,
  status: Memory["status"],
  overrides: Partial<Memory> = {},
): Memory {
  return {
    id,
    tier: "project",
    scope_key: "/repo/a",
    kind: "decision",
    content: `memory ${id}`,
    source_session: null,
    confidence: 0,
    usage_count: 0,
    last_used: null,
    created_at: id,
    knowledge_type: "decision",
    status,
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

const memories: Memory[] = [
  memory(7, "rejected"),
  memory(6, "archived"),
  memory(5, "verified"),
  memory(4, "legacy_unverified", { scope_key: "/repo/b", content: "Legacy convention" }),
  memory(3, "stale", { content: "JWT decision" }),
  memory(2, "pending_review"),
  memory(1, "candidate", { dormant: true }),
];

describe("memory review filtering", () => {
  it("summarizes actionable stages without calling verified items injected", () => {
    expect(summarizeMemoryActivation(memories)).toEqual({
      actionable: 3,
      candidate: 1,
      pendingReview: 1,
      stale: 1,
      legacyUnverified: 1,
      verified: 1,
    });
  });

  it("combines actionable status, project scope, and case-insensitive search", () => {
    expect(
      filterMemoryReview(memories, {
        filter: "actionable",
        query: "",
        scopeKey: null,
      }).map(({ id }) => id),
    ).toEqual([4, 3, 2]);
    expect(
      filterMemoryReview(memories, {
        filter: "actionable",
        query: "jwt",
        scopeKey: "/repo/a",
      }).map(({ id }) => id),
    ).toEqual([3]);
  });

  it("supports exact lifecycle and dormant filters", () => {
    expect(
      filterMemoryReview(memories, {
        filter: "legacy_unverified",
        query: "",
        scopeKey: null,
      }).map(({ id }) => id),
    ).toEqual([4]);
    expect(
      filterMemoryReview(memories, {
        filter: "dormant",
        query: "",
        scopeKey: null,
      }).map(({ id }) => id),
    ).toEqual([1]);
  });

  it("separates active work from archived and rejected history", () => {
    expect(
      filterMemoryReview(memories, {
        filter: "active",
        query: "",
        scopeKey: null,
      }).map(({ id }) => id),
    ).toEqual([5, 4, 3, 2, 1]);
    expect(
      filterMemoryReview(memories, {
        filter: "archived",
        query: "",
        scopeKey: null,
      }).map(({ id }) => id),
    ).toEqual([6]);
  });
});

describe("memory review filter counts", () => {
  it("검색·범위를 먼저 적용한 뒤 상태별로 센다 — 누르면 나올 수를 병기해야 한다", () => {
    const counts = countMemoryReviewFilters(memories, { query: "", scopeKey: null });

    expect(counts.all).toBe(7);
    // 활성은 보관·거절을 뺀다 — 정리한 것이 목록에 남아 있으면 정리한 것처럼 보이지 않는다.
    expect(counts.active).toBe(5);
    expect(counts.archived).toBe(1);
    // dormant candidate는 검토 대상에서 빠지고 후보·휴면에는 남는다.
    expect(counts.actionable).toBe(3);
    expect(counts.candidate).toBe(1);
    expect(counts.dormant).toBe(1);
    expect(counts.legacy_unverified).toBe(1);
    expect(counts.verified).toBe(1);
  });

  it("범위를 좁히면 그 프로젝트 것만 센다", () => {
    const counts = countMemoryReviewFilters(memories, { query: "", scopeKey: "/repo/b" });

    expect(counts.all).toBe(1);
    expect(counts.legacy_unverified).toBe(1);
    expect(counts.stale).toBe(0);
  });

  it("검색어를 적용한 개수를 센다 — 검색 중에는 빈 세그먼트가 사라져야 한다", () => {
    const counts = countMemoryReviewFilters(memories, { query: "jwt", scopeKey: null });

    expect(counts.all).toBe(1);
    expect(counts.stale).toBe(1);
    expect(counts.verified).toBe(0);
  });
});
