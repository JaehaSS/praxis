import type { ReactElement } from "react";
import { FilterSegment, type FilterSegmentItem } from "./FilterSegment";
import type { MemoryActivationSummary, MemoryReviewFilter } from "./memory-review";

interface Props {
  activation: boolean;
  summary: MemoryActivationSummary;
  query: string;
  filter: MemoryReviewFilter;
  scopeKey: string | null;
  scopes: string[];
  /** 검색·범위를 적용한 뒤의 상태별 개수 — 세그먼트에 병기한다. */
  filterCounts: Record<MemoryReviewFilter, number>;
  filteredCount: number;
  totalCount: number;
  visibleCount: number;
  onQueryChange: (value: string) => void;
  onFilterChange: (value: MemoryReviewFilter) => void;
  onScopeChange: (value: string | null) => void;
}

/** 세그먼트 순서 — 수명주기를 왼쪽에서 오른쪽으로 읽고, 범위를 여는 칸을 양 끝에 둔다. */
const FILTERS: Array<{ value: MemoryReviewFilter; label: string; title?: string }> = [
  {
    value: "active",
    label: "활성",
    title: "보관·거절을 제외한 검토 및 승인 상태",
  },
  { value: "actionable", label: "검토 대상" },
  { value: "candidate", label: "후보" },
  { value: "pending_review", label: "검토 중" },
  { value: "stale", label: "stale" },
  { value: "legacy_unverified", label: "이관 대기" },
  { value: "verified", label: "승인 완료" },
  { value: "dormant", label: "휴면" },
  { value: "archived", label: "보관됨", title: "본문·근거·이력이 유지된 보관 항목" },
  { value: "all", label: "전체" },
];

export function MemoryReviewControls(props: Props): ReactElement {
  const segments: FilterSegmentItem[] = FILTERS.map((item) => ({
    value: item.value,
    label: item.label,
    count: props.filterCounts[item.value] ?? 0,
    title: item.title,
  }));

  return (
    <div className="mb-3 space-y-2.5">
      {props.activation && (
        <MemoryActivationGuide
          summary={props.summary}
          onShowLegacy={() => props.onFilterChange("legacy_unverified")}
        />
      )}
      {props.filter === "legacy_unverified" && (
        <LegacyMigrationGuide count={props.filteredCount} />
      )}
      <div className="rounded-lg border border-border bg-surface p-3">
        <div className="flex flex-wrap gap-2">
          <input
            aria-label="메모리 검색"
            className="min-w-56 flex-1 rounded border border-border bg-bg px-2 py-1.5 text-sm outline-none focus:border-primary"
            placeholder="내용·상태·프로젝트 검색"
            value={props.query}
            onChange={(event) => props.onQueryChange(event.target.value)}
          />
          <select
            aria-label="프로젝트 범위"
            className="max-w-full rounded border border-border bg-bg px-2 py-1.5 text-sm outline-none focus:border-primary"
            value={props.scopeKey ?? ""}
            onChange={(event) => props.onScopeChange(event.target.value || null)}
          >
            <option value="">전체 프로젝트</option>
            {props.scopes.map((scope) => (
              <option key={scope} value={scope}>
                {scope}
              </option>
            ))}
          </select>
        </div>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <FilterSegment
            label="메모리 상태 필터"
            items={segments}
            value={props.filter}
            onChange={(value) => props.onFilterChange(value as MemoryReviewFilter)}
            alwaysVisible={["active", "all"]}
          />
          <span className="ml-auto text-xs text-text-muted">
            {formatCount(props.visibleCount)} / {formatCount(props.filteredCount)}건 표시 · 전체{" "}
            {formatCount(props.totalCount)}건
          </span>
        </div>
      </div>
    </div>
  );
}

function MemoryActivationGuide({
  summary,
  onShowLegacy,
}: {
  summary: MemoryActivationSummary;
  onShowLegacy: () => void;
}): ReactElement {
  return (
    <section
      aria-label="Memory 활성화 안내"
      className="rounded-lg border border-primary/40 bg-primary/5 p-3"
    >
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h2 className="text-sm font-medium text-text">첫 versioned memory 활성화</h2>
        <span className="text-xs text-text-muted">
          전체 승인 완료 {formatCount(summary.verified)} · 비휴면 검토 대상{" "}
          {formatCount(summary.actionable)}
        </span>
      </div>
      <ol className="mt-2 grid gap-1 text-xs leading-relaxed text-text-secondary sm:grid-cols-3">
        <li>1. 검색과 프로젝트 필터로 다시 쓸 가치가 명확한 후보를 고릅니다.</li>
        <li>2. 본문과 근거를 검토하고, 사실을 직접 확인한 경우에만 승인합니다.</li>
        <li>
          3. <strong className="font-medium text-primary-bright">직접 확인 후 승인</strong>하면
          receipt가 생기며, 이후 freshness·relevance 게이트를 통과해 선택되면 versioned ledger에
          기록됩니다.
        </li>
      </ol>
      {summary.legacyUnverified > 0 && (
        <button
          type="button"
          className="mt-2 text-xs text-primary-bright hover:text-text"
          onClick={onShowLegacy}
        >
          이관 대기 {formatCount(summary.legacyUnverified)}건 보기
        </button>
      )}
    </section>
  );
}

function LegacyMigrationGuide({ count }: { count: number }): ReactElement {
  return (
    <section
      aria-label="legacy memory 이관 안내"
      className="rounded-lg border border-status-awaiting/40 bg-status-awaiting/5 p-3"
    >
      <h3 className="text-sm font-medium text-text">
        legacy 이관 대기 {formatCount(count)}건
      </h3>
      <p className="mt-1 text-xs leading-relaxed text-text-secondary">
        다시 쓸 항목은 근거를 열어 직접 검토·승인하고, 가치가 낮은 항목은 체크한 뒤 선택
        보관합니다. 보관해도 본문·근거·이력은 남으며, 사람 판단 없이 일괄 승인하지 않습니다.
      </p>
    </section>
  );
}

function formatCount(value: number): string {
  return value.toLocaleString("ko-KR");
}
