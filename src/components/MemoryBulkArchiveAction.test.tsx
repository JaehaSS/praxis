import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MemoryBulkArchiveAction } from "./MemoryBulkArchiveAction";

describe("MemoryBulkArchiveAction", () => {
  it("names the exact current-result scope and audit-safe action", () => {
    const html = renderToStaticMarkup(
      <MemoryBulkArchiveAction count={12} busy={false} onArchive={() => undefined} />,
    );

    expect(html).toContain("현재 결과 12건 보관");
    expect(html).toContain("현재 검색·프로젝트·상태 필터 결과");
  });

  it("hides when no current result can be archived", () => {
    expect(
      renderToStaticMarkup(
        <MemoryBulkArchiveAction count={0} busy={false} onArchive={() => undefined} />,
      ),
    ).toBe("");
  });
});
