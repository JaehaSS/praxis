import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DiffFileHeader, DiffModeToggle, SplitDiff } from "./DiffPresentation";

describe("Diff presentation", () => {
  it("renders Korean unified and split controls with explicit pressed state", () => {
    const html = renderToStaticMarkup(<DiffModeToggle mode="split" onChange={() => {}} />);
    expect(html).toContain("통합");
    expect(html).toContain("분할");
    expect(html).toContain('aria-pressed="true"');
  });

  it("shows per-file addition and deletion totals", () => {
    const html = renderToStaticMarkup(
      <DiffFileHeader
        file={{
          path: "src/app.ts",
          status: "M",
          patch: "@@ -1 +1,2 @@\n-old\n+new\n+next",
        }}
      />,
    );
    expect(html).toContain("+2");
    expect(html).toContain("−1");
  });

  it("explains rename-only changes instead of leaving split view blank", () => {
    const html = renderToStaticMarkup(
      <SplitDiff patch={"similarity index 100%\nrename from old.ts\nrename to new.ts"} />,
    );
    expect(html).toContain("파일 이름이 변경되었습니다");
    expect(html).toContain("old.ts → new.ts");
  });
});
