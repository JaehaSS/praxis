import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MemoryReadinessNote } from "./MemoryReadinessNote";

describe("MemoryReadinessNote", () => {
  it("renders a blocking reason as an actionable diagnostic", () => {
    const html = renderToStaticMarkup(
      <MemoryReadinessNote
        readiness={{
          tone: "blocked",
          label: "근거 차단 1건",
          detail: "재검증 후 필요하면 새 버전으로 편집하세요.",
          canApprove: false,
        }}
      />,
    );

    expect(html).toContain('aria-label="주입 준비 상태"');
    expect(html).toContain("근거 차단 1건");
    expect(html).toContain("새 버전으로 편집");
  });

  it("does not describe a ready candidate as already injected", () => {
    const html = renderToStaticMarkup(
      <MemoryReadinessNote
        readiness={{
          tone: "ready",
          label: "주입 후보",
          detail: "관련 작업에서 선택될 때만 적용됩니다.",
          canApprove: false,
        }}
      />,
    );

    expect(html).toContain("주입 후보");
    expect(html).not.toContain("주입 완료");
  });
});
