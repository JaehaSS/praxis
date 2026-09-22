import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { DiffHunk, RematchedAnnotation } from "../lib/ipc";
import { AnnotatedUnifiedDiff } from "./DiffAnnotations";

const hunk: DiffHunk = {
  id: "h1",
  path: "src/auth/login.ts",
  old_range: [9, 2],
  new_range: [9, 3],
  lines: [
    { kind: "context", text: "function login() {" },
    { kind: "add", text: "if (!token) throw new AuthError()" },
    { kind: "context", text: "}" },
  ],
  protected: false,
    committed: false,
  risk: "low",
};

function annotation(overrides: Partial<RematchedAnnotation> = {}): RematchedAnnotation {
  return {
    id: "ann-1",
    task_id: 1,
    hunk_id: "h1",
    path: "src/auth/login.ts",
    line: 10,
    side: "new",
    body_md: "에러 코드는 상수로 빼줘",
    status: "draft",
    created_at: 1000,
    matched_hunk_id: "h1",
    orphaned: false,
    ...overrides,
  };
}

describe("AnnotatedUnifiedDiff", () => {
  it("renders hunk lines with the added text and an existing comment thread", () => {
    const html = renderToStaticMarkup(
      <AnnotatedUnifiedDiff
        hunks={[hunk]}
        annotations={[annotation()]}
        onCreate={async () => {}}
        onUpdateBody={async () => {}}
      />,
    );

    expect(html).toContain("if (!token) throw new AuthError()");
    expect(html).toContain("에러 코드는 상수로 빼줘");
    expect(html).toContain("초안");
    expect(html).toContain("💬");
  });

  it("shows an orphaned banner without dropping the annotation", () => {
    const html = renderToStaticMarkup(
      <AnnotatedUnifiedDiff
        hunks={[hunk]}
        annotations={[annotation({ id: "ann-2", orphaned: true, matched_hunk_id: null, status: "sent" })]}
        onCreate={async () => {}}
        onUpdateBody={async () => {}}
      />,
    );

    expect(html).toContain("고아 주석 1건");
    expect(html).toContain("src/auth/login.ts:10");
  });

  it("renders a hunk checkbox that is checked when selected, and disabled with a reason when protected", () => {
    const protectedHunk: DiffHunk = { ...hunk, id: "h2", protected: true };
    const html = renderToStaticMarkup(
      <AnnotatedUnifiedDiff
        hunks={[hunk, protectedHunk]}
        annotations={[]}
        onCreate={async () => {}}
        onUpdateBody={async () => {}}
        selectedHunkIds={new Set(["h1"])}
        onToggleHunk={() => {}}
      />,
    );

    expect(html).toContain('type="checkbox"');
    expect(html).toContain("checked=\"\"");
    expect(html).toContain("disabled=\"\"");
    expect(html).toContain("부분 적용으로 유지할 수 없습니다");
  });

  it("omits checkboxes entirely when selectedHunkIds is not provided", () => {
    const html = renderToStaticMarkup(
      <AnnotatedUnifiedDiff hunks={[hunk]} annotations={[]} onCreate={async () => {}} onUpdateBody={async () => {}} />,
    );

    expect(html).not.toContain('type="checkbox"');
  });

  it("shows the omitted line count between distant hunks", () => {
    const later: DiffHunk = {
      ...hunk,
      id: "h-later",
      old_range: [30, 2],
      new_range: [31, 2],
    };
    const html = renderToStaticMarkup(
      <AnnotatedUnifiedDiff
        hunks={[hunk, later]}
        annotations={[]}
        onCreate={async () => {}}
        onUpdateBody={async () => {}}
      />,
    );

    expect(html).toContain("19개 변경되지 않은 줄");
  });
});
