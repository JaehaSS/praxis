import { expect, it } from "vitest";
import { buildWikiSearchIndex, searchWikiPages, type SearchablePage } from "./wiki-search";

const page = (id: string, extra: Partial<SearchablePage> = {}): SearchablePage => ({
  id, title: id, path: id, aliases: [], body: "", ...extra,
});

const search = (pages: SearchablePage[], query: string) =>
  searchWikiPages(buildWikiSearchIndex(pages), pages, query);

it("ranks a title hit above an alias, a path, and a body hit", () => {
  const pages = [
    page("d.md", { title: "무관", body: "여기에 그래프 이야기가 있다" }),
    page("c.md", { title: "무관", path: "그래프/c.md" }),
    page("b.md", { title: "무관", aliases: ["그래프 별칭"] }),
    page("a.md", { title: "그래프 개요" }),
  ];

  expect(search(pages, "그래프").map(result => [result.page.id, result.match?.field]))
    .toEqual([["a.md", "title"], ["b.md", "alias"], ["c.md", "path"], ["d.md", "body"]]);
});

it("puts an earlier hit first inside the same field and keeps the given order on a full tie", () => {
  const pages = [page("b.md", { title: "위키 그래프" }), page("a.md", { title: "그래프 위키" }), page("c.md", { title: "위키 그래프" })];

  expect(search(pages, "그래프").map(result => result.page.id)).toEqual(["a.md", "b.md", "c.md"]);
});

it("splits a body hit into three pieces so the caller never computes an offset", () => {
  const body = `${"가".repeat(60)}표적${"나".repeat(90)}`;
  const [result] = search([page("a.md", { title: "무관", body })], "표적");

  expect(result.match).toEqual({
    field: "body",
    before: `…${"가".repeat(24)}`,
    text: "표적",
    after: `${"나".repeat(56)}…`,
  });
});

it("keeps the whole field and drops the ellipsis when the hit is not in the body", () => {
  const [result] = search([page("a.md", { title: "위키 그래프 개요" })], "그래프");

  expect(result.match).toEqual({ field: "title", before: "위키 ", text: "그래프", after: " 개요" });
});

it("presses newlines into single spaces so a snippet stays on one line", () => {
  const [result] = search([page("a.md", { title: "무관", body: "첫 줄\n\n표적\n  마지막" })], "표적");

  expect(result.match?.before).toBe("첫 줄 ");
  expect(result.match?.after).toBe(" 마지막");
});

it("matches case-insensitively and shows the original casing back", () => {
  const [result] = search([page("a.md", { title: "Wiki Graph" })], "GRAPH");

  expect(result.match?.text).toBe("Graph");
});

it("matches the same Korean text whether it arrives composed or decomposed", () => {
  const pages = [page("a.md", { title: "위키".normalize("NFD") })];

  expect(search(pages, "위키")).toHaveLength(1);
  expect(search(pages, "위키".normalize("NFD"))).toHaveLength(1);
});

it("never cuts a surrogate pair in half at a snippet edge", () => {
  // 이모지는 UTF-16 두 칸을 쓴다 — 창 경계가 그 한가운데에 떨어지도록 앞을 25칸으로 맞춘다.
  const body = `${"가".repeat(23)}🌊표적`;
  const [result] = search([page("a.md", { title: "무관", body })], "표적");

  expect(result.match?.before).toBe(`…${"가".repeat(22)}🌊`);
  expect([...(result.match?.before ?? "")].every(char => char.codePointAt(0)! < 0xd800 || char.codePointAt(0)! > 0xdfff)).toBe(true);
});

it("returns every page untouched for a blank query and finds nothing across a field boundary", () => {
  const pages = [page("b.md", { title: "나" }), page("a.md", { title: "가" })];

  expect(search(pages, "   ").map(result => [result.page.id, result.match])).toEqual([["b.md", null], ["a.md", null]]);
  // 예전에는 필드를 이어 붙여 한 번에 찾아서 "가 a.md" 같은 경계 걸침이 맞았다.
  expect(search(pages, "가 a.md")).toEqual([]);
});
