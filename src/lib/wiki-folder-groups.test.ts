import { expect, it } from "vitest";
import { folderColor, folderGroups, folderLabel, folderSlotOf, topFolder } from "./wiki-folder-groups";

// 폴더마다 문서 수를 다르게 둔다 — 순위가 건수만으로 정해져야 동점 규칙과 뒤섞이지 않는다.
const paths = [
  "연구/a.md", "연구/b.md", "연구/c.md", "연구/d.md",
  "일지/e.md", "일지/f.md", "일지/g.md",
  "메모/h.md", "메모/i.md",
  "잡동/j.md",
  "루트.md",
];

it("gives the three largest folders a colour and leaves the rest without one", () => {
  const groups = folderGroups(paths);

  expect(groups.map(group => [group.folder, group.slot])).toEqual([["연구", 0], ["일지", 1], ["메모", 2]]);
  expect(folderSlotOf(groups, "연구/a.md")).toBe(0);
  expect(folderSlotOf(groups, "잡동/j.md")).toBeNull();
  expect(folderSlotOf(groups, "루트.md")).toBeNull();
});

it("breaks a document-count tie by name so the same vault always gets the same colours", () => {
  const first = folderGroups(["나/a.md", "가/b.md", "다/c.md", "라/d.md"]);
  const second = folderGroups(["라/d.md", "다/c.md", "가/b.md", "나/a.md"]);

  expect(first).toEqual(second);
  expect(first.map(group => group.folder)).toEqual(["가", "나", "다"]);
});

it("keeps a folder's colour when a filter removes documents from it", () => {
  const all = folderGroups(paths);
  // 연구 폴더가 한 건만 남도록 걸러도, 배정은 전체로 계산하므로 색이 그대로여야 한다.
  const survivors = ["연구/a.md", "일지/e.md", "메모/h.md"];

  expect(survivors.map(path => folderSlotOf(all, path))).toEqual([0, 1, 2]);
});

it("names the root folder and points each slot at its own theme variable", () => {
  expect(topFolder("연구/하위/a.md")).toBe("연구");
  expect(folderLabel("")).toBe("(루트)");
  expect([0, 1, 2, null].map(folderColor)).toEqual(["var(--c-cat-1)", "var(--c-cat-2)", "var(--c-cat-3)", "var(--c-text-2)"]);
});
