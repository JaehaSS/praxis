import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MemoryReviewControls } from "./MemoryReviewControls";

const summary = {
  actionable: 1_077,
  candidate: 857,
  pendingReview: 0,
  stale: 0,
  legacyUnverified: 220,
  verified: 0,
};

/** 세그먼트는 0건이면 렌더하지 않는다 — 라벨 존재를 보는 케이스는 전부 양수를 준다. */
const filterCounts = {
  active: 1_075,
  archived: 4,
  all: 1_079,
  actionable: 1_077,
  candidate: 857,
  pending_review: 3,
  stale: 71,
  legacy_unverified: 220,
  verified: 4,
  dormant: 2,
};

const callbacks = {
  onQueryChange: () => undefined,
  onFilterChange: () => undefined,
  onScopeChange: () => undefined,
};

describe("MemoryReviewControls", () => {
  it("explains the first verified-memory loop without claiming injection already happened", () => {
    const html = renderToStaticMarkup(
      <MemoryReviewControls
        activation
        summary={summary}
        query=""
        filter="actionable"
        scopeKey="/repo/a"
        scopes={["/repo/a", "/repo/b"]}
        filteredCount={599}
        totalCount={1_079}
        visibleCount={50}
        filterCounts={filterCounts}
        {...callbacks}
      />,
    );

    expect(html).toContain("첫 versioned memory 활성화");
    expect(html).toContain("전체 승인 완료 0");
    expect(html).toContain("검토 대상 1,077");
    expect(html).toContain("직접 확인 후 승인");
    expect(html).toContain("freshness·relevance");
    expect(html).toContain("선택되면 versioned ledger");
    expect(html).not.toContain("주입 완료");
  });

  it("renders search, lifecycle filters, scope, and progressive counts", () => {
    const html = renderToStaticMarkup(
      <MemoryReviewControls
        activation={false}
        summary={summary}
        query="jwt"
        filter="stale"
        scopeKey={null}
        scopes={["/repo/a"]}
        filteredCount={71}
        totalCount={1_079}
        visibleCount={50}
        filterCounts={filterCounts}
        {...callbacks}
      />,
    );

    expect(html).toContain('aria-label="메모리 검색"');
    expect(html).toContain('value="jwt"');
    expect(html).toContain("활성");
    expect(html).toContain("검토 대상");
    expect(html).toContain("후보");
    expect(html).toContain("검토 중");
    expect(html).toContain("stale");
    expect(html).toContain("이관 대기");
    expect(html).toContain("승인 완료");
    expect(html).toContain("보관됨");
    expect(html).toContain("50 / 71건 표시");
    expect(html).toContain("전체 1,079건");
  });

  it("keeps ordinary browse mode free of activation coaching", () => {
    const html = renderToStaticMarkup(
      <MemoryReviewControls
        activation={false}
        summary={summary}
        query=""
        filter="all"
        scopeKey={null}
        scopes={[]}
        filteredCount={1_079}
        totalCount={1_079}
        visibleCount={50}
        filterCounts={filterCounts}
        {...callbacks}
      />,
    );

    expect(html).not.toContain("첫 versioned memory 활성화");
  });

  it("explains the dedicated legacy migration actions without bulk approval", () => {
    const html = renderToStaticMarkup(
      <MemoryReviewControls
        activation
        summary={summary}
        query=""
        filter="legacy_unverified"
        scopeKey={null}
        scopes={[]}
        filteredCount={220}
        totalCount={1_079}
        visibleCount={50}
        filterCounts={filterCounts}
        {...callbacks}
      />,
    );

    expect(html).toContain("legacy 이관 대기 220건");
    expect(html).toContain("근거를 열어 직접 검토");
    expect(html).toContain("선택 보관");
    expect(html).toContain("일괄 승인하지 않습니다");
  });
});
