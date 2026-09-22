import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  initialReviewFilter,
  initialReviewScope,
  KINDS,
  kindColor,
  MemoryView,
} from "./MemoryView";

describe("MemoryView kinds", () => {
  it("exposes handoff-axis kinds in the selector list", () => {
    expect(KINDS).toContain("abandoned");
    expect(KINDS).toContain("pitfall");
  });

  it("assigns a distinct color to every kind", () => {
    for (const k of KINDS) {
      expect(kindColor[k], `kindColor missing: ${k}`).toBeTruthy();
    }
    expect(new Set(KINDS.map((k) => kindColor[k])).size).toBe(KINDS.length);
  });
});

describe("MemoryView entry mode", () => {
  it("starts the Outcome Insights entry in scoped activation mode", () => {
    expect(initialReviewFilter(true)).toBe("actionable");
    expect(initialReviewScope(true, "/repo/a")).toBe("/repo/a");

    const html = renderToStaticMarkup(
      <MemoryView activation preferredScope="/repo/a" />,
    );

    expect(html).toContain("첫 versioned memory 활성화");
    expect(html).toContain('<option value="/repo/a" selected="">/repo/a</option>');
  });

  it("keeps direct navigation in ordinary browse mode", () => {
    expect(initialReviewFilter(false)).toBe("active");
    // 직접 들어오면 preferredScope가 있어도 좁히지 않는다.
    expect(initialReviewScope(false, "/repo/a")).toBeNull();

    const html = renderToStaticMarkup(<MemoryView />);

    expect(html).not.toContain("첫 versioned memory 활성화");
  });
});

describe("MemoryView tabs", () => {
  it("전환 컨트롤을 Tabs 문법으로 낸다 — 면을 바꾸는 것은 범위 필터와 다른 언어다", () => {
    const html = renderToStaticMarkup(<MemoryView />);

    expect(html).toContain('role="tablist" aria-label="메모리 화면"');
    // 활성 탭: 하단선 2px + primaryBright. pill 배경(bg-raised)이 아니다.
    expect(html).toMatch(
      /aria-selected="true"[^>]*class="[^"]*border-primary[^"]*text-primary-bright[^"]*"[^>]*>메모리</,
    );
    expect(html).toMatch(
      /aria-selected="false"[^>]*class="[^"]*border-transparent[^"]*"[^>]*>자기개선</,
    );
    // roving tabIndex — Tab이 탭마다 멈추지 않는다.
    expect(html).toMatch(/aria-selected="true"[^>]*tabindex="0"/);
    expect(html).toMatch(/aria-selected="false"[^>]*tabindex="-1"/);
  });
});
