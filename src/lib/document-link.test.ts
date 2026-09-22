import { describe, expect, it } from "vitest";
import { resolveDocumentLink } from "./document-link";

describe("resolveDocumentLink", () => {
  const source = "docs/discovery/example.md";

  it("resolves a document-relative target inside the workspace", () => {
    expect(resolveDocumentLink("../../DESIGN%20%ED%95%9C%EA%B8%80.md:4#intro", source)).toEqual({
      kind: "file",
      path: "DESIGN 한글.md",
    });
  });

  it("keeps HTTP(S) links as browser targets", () => {
    expect(resolveDocumentLink("https://example.com/docs#part", source)).toEqual({
      kind: "url",
      url: "https://example.com/docs#part",
    });
  });

  it("does not make anchors, unsupported schemes, decoded controls, tilde paths, or workspace escapes into file targets", () => {
    for (const link of ["#part", "javascript:alert(1)", "file:///Users/test/private.md", "file%00.md", "~/private.md", "../../../private.md"]) {
      expect(resolveDocumentLink(link, source)).toBeNull();
    }
  });

  it("resolves an absolute link that sits inside the root", () => {
    expect(resolveDocumentLink("/work/root/docs/a.md:3#x", source, "/work/root")).toEqual({
      kind: "file",
      path: "docs/a.md",
    });
    expect(resolveDocumentLink("/work/root/docs/a.md", source, "/work/root/")).toEqual({
      kind: "file",
      path: "docs/a.md",
    });
    expect(resolveDocumentLink("/work/root/docs/%ED%95%9C%EA%B8%80.md", source, "/work/root")).toEqual({
      kind: "file",
      path: "docs/한글.md",
    });
  });

  it("does not open absolute links outside the root, or any absolute link without one", () => {
    for (const link of ["/etc/passwd", "/work/root/../etc/passwd", "/work/root2/a.md"]) {
      expect(resolveDocumentLink(link, source, "/work/root")).toBeNull();
    }
    expect(resolveDocumentLink("/work/root/docs/a.md", source)).toBeNull();
  });

  it("rejects a source path that is not already workspace-relative", () => {
    expect(resolveDocumentLink("notes.md", "/Users/test/example.md")).toBeNull();
    expect(resolveDocumentLink("notes.md", "docs/../example.md")).toBeNull();
  });
});
