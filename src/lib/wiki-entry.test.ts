import { expect, it } from "vitest";
import { entryPage } from "./wiki-entry";
import type { WikiPage } from "./wiki-workspace-ipc";

const page = (id: string, backlinks: string[] = []): WikiPage => ({
  id, path: id, title: id, aliases: [], tags: [], type: "page", status: "", scope: "",
  body: "", source_prefix: "", outgoing: [], backlinks, sha256: `hash-${id}`,
});

it("prefers the exact configured path", () => {
  const pages = [page("wiki/다른.md", ["a.md", "b.md"]), page("wiki/위키-시작.md")];

  expect(entryPage(pages, "wiki/위키-시작.md")).toBe("wiki/위키-시작.md");
});

it("matches a bare file name at the shallowest place so the wiki folder can move", () => {
  const pages = [page("문서/기술-위키/wiki/위키-시작.md"), page("wiki/위키-시작.md")];

  expect(entryPage(pages, "위키-시작.md")).toBe("wiki/위키-시작.md");
  // 경로로 적어도 이름 일치로 내려온다 — 설정과 실제 위치가 어긋나도 위키가 열린다.
  expect(entryPage(pages, "없는곳/위키-시작.md")).toBe("wiki/위키-시작.md");
});

it("falls back to the most linked document, then to a stable first document", () => {
  const hub = [page("a.md"), page("허브.md", ["a.md", "b.md"]), page("b.md", ["a.md"])];
  expect(entryPage(hub, "위키-시작.md")).toBe("허브.md");

  const flat = [page("b.md"), page("a.md")];
  expect(entryPage(flat, "위키-시작.md")).toBe("a.md");
});

it("normalizes the configured name to NFC and ignores case of the extension", () => {
  const pages = [page("wiki/위키-시작.MD")];

  expect(entryPage(pages, "위키-시작.md".normalize("NFD"))).toBe("wiki/위키-시작.MD");
});

it("has nothing to open in an empty vault", () => {
  expect(entryPage([], "위키-시작.md")).toBeNull();
});
