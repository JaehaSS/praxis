import type { Memory } from "../lib/ipc";

export type MemoryReviewFilter =
  | "all"
  | "active"
  | "actionable"
  | "candidate"
  | "pending_review"
  | "verified"
  | "stale"
  | "legacy_unverified"
  | "dormant"
  | "archived";

export interface MemoryReviewCriteria {
  filter: MemoryReviewFilter;
  query: string;
  scopeKey: string | null;
}

export interface MemoryActivationSummary {
  actionable: number;
  candidate: number;
  pendingReview: number;
  stale: number;
  legacyUnverified: number;
  verified: number;
}

const ACTIONABLE_STATUSES = new Set<Memory["status"]>([
  "candidate",
  "pending_review",
  "stale",
  "legacy_unverified",
]);

export function summarizeMemoryActivation(memories: Memory[]): MemoryActivationSummary {
  const summary: MemoryActivationSummary = {
    actionable: 0,
    candidate: 0,
    pendingReview: 0,
    stale: 0,
    legacyUnverified: 0,
    verified: 0,
  };
  for (const memory of memories) {
    if (ACTIONABLE_STATUSES.has(memory.status) && !memory.dormant) summary.actionable += 1;
    if (memory.status === "candidate") summary.candidate += 1;
    if (memory.status === "pending_review") summary.pendingReview += 1;
    if (memory.status === "stale") summary.stale += 1;
    if (memory.status === "legacy_unverified") summary.legacyUnverified += 1;
    if (memory.status === "verified") summary.verified += 1;
  }
  return summary;
}

export function filterMemoryReview(
  memories: Memory[],
  criteria: MemoryReviewCriteria,
): Memory[] {
  const query = criteria.query.trim().toLocaleLowerCase();
  return memories.filter(
    (memory) =>
      matchesScope(memory, criteria.scopeKey, query) &&
      matchesFilter(memory, criteria.filter),
  );
}

/** 세그먼트 순서 — 수명주기를 왼쪽에서 오른쪽으로 읽는다. `all`은 범위를 여는 칸이라 맨 앞. */
const REVIEW_FILTERS: MemoryReviewFilter[] = [
  "active",
  "actionable",
  "candidate",
  "pending_review",
  "stale",
  "legacy_unverified",
  "verified",
  "dormant",
  "archived",
  "all",
];

/**
 * 필터 세그먼트에 병기할 개수.
 *
 * 검색어와 프로젝트 범위를 **먼저** 적용한 뒤 상태별로 센다 — 누르면 실제로 나올 수가
 * 아니면 병기하는 의미가 없다(DESIGN.md `components.FilterSegment.count`).
 */
export function countMemoryReviewFilters(
  memories: Memory[],
  criteria: Omit<MemoryReviewCriteria, "filter">,
): Record<MemoryReviewFilter, number> {
  const query = criteria.query.trim().toLocaleLowerCase();
  const counts = Object.fromEntries(
    REVIEW_FILTERS.map((filter) => [filter, 0]),
  ) as Record<MemoryReviewFilter, number>;
  for (const memory of memories) {
    if (!matchesScope(memory, criteria.scopeKey, query)) continue;
    for (const filter of REVIEW_FILTERS) {
      if (matchesFilter(memory, filter)) counts[filter] += 1;
    }
  }
  return counts;
}

/** query는 호출측에서 미리 trim·소문자화한 값을 넘긴다 — 세그먼트 카운트가 N번 반복 호출한다. */
function matchesScope(memory: Memory, scopeKey: string | null, query: string): boolean {
  if (scopeKey !== null && memory.scope_key !== scopeKey) return false;
  if (!query) return true;
  return searchableText(memory).includes(query);
}

function matchesFilter(memory: Memory, filter: MemoryReviewFilter): boolean {
  if (filter === "all") return true;
  if (filter === "active") {
    return memory.status !== "archived" && memory.status !== "rejected";
  }
  if (filter === "actionable") return ACTIONABLE_STATUSES.has(memory.status) && !memory.dormant;
  if (filter === "dormant") return memory.dormant;
  return memory.status === filter;
}

function searchableText(memory: Memory): string {
  return [
    memory.content,
    memory.knowledge_type,
    memory.status,
    memory.scope_key ?? "",
    memory.source_session ?? "",
  ]
    .join("\n")
    .toLocaleLowerCase();
}
