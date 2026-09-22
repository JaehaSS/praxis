import { describe, expect, it } from "vitest";
import type { FsNode } from "../../lib/ipc";
import {
  initialExpanded,
  keyAction,
  sortNodes,
  toNavRows,
  typeAheadIndex,
  visibleRows,
  type NavRow,
} from "./tree-nav";

const dir = (name: string, path: string, children: FsNode[] = []): FsNode => ({
  name,
  path,
  is_dir: true,
  children,
});
const file = (name: string, path: string): FsNode => ({
  name,
  path,
  is_dir: false,
  children: [],
});

const TREE: FsNode[] = [
  dir("src", "src", [
    dir("lib", "src/lib", [file("a.ts", "src/lib/a.ts")]),
    file("App.tsx", "src/App.tsx"),
  ]),
  dir(".github", ".github", [file("ci.yml", ".github/ci.yml")]),
  file("README.md", "README.md"),
  file(".gitignore", ".gitignore"),
];

describe("sortNodes", () => {
  it("디렉터리를 파일보다 앞에 두고 각 그룹은 이름순", () => {
    const sorted = sortNodes([
      file("z.ts", "z.ts"),
      dir("b", "b"),
      file("a.ts", "a.ts"),
      dir("a", "a"),
    ]);
    expect(sorted.map((n) => n.name)).toEqual(["a", "b", "a.ts", "z.ts"]);
  });
});

describe("visibleRows", () => {
  it("접힌 디렉터리의 자식은 나오지 않는다", () => {
    const rows = visibleRows(TREE, new Set(), true);
    expect(rows.map((r) => r.node.name)).toEqual([".github", "src", ".gitignore", "README.md"]);
  });

  it("펼친 가지만 depth를 늘려 이어 붙인다", () => {
    const rows = visibleRows(TREE, new Set(["src"]), true);
    expect(rows.map((r) => [r.node.name, r.depth])).toEqual([
      [".github", 0],
      ["src", 0],
      ["lib", 1],
      ["App.tsx", 1],
      [".gitignore", 0],
      ["README.md", 0],
    ]);
  });

  it("숨김을 끄면 도트 항목이 통째로 빠진다 — 그 자식도 따라 사라진다", () => {
    const rows = visibleRows(TREE, new Set([".github", "src"]), false);
    expect(rows.map((r) => r.node.name)).toEqual(["src", "lib", "App.tsx", "README.md"]);
  });
});

describe("initialExpanded", () => {
  it("루트 바로 아래 디렉터리만 펼친다", () => {
    expect(initialExpanded(TREE)).toEqual(new Set(["src", ".github"]));
  });
});

describe("keyAction", () => {
  const rows = () => toNavRows(visibleRows(TREE, new Set(["src"]), true));

  it("커서가 없으면 어떤 이동키든 첫 행을 잡는다", () => {
    expect(keyAction("ArrowDown", rows(), -1, new Set())).toEqual({ kind: "focus", index: 0 });
    expect(keyAction("ArrowUp", rows(), -1, new Set())).toEqual({ kind: "focus", index: 0 });
  });

  it("위아래 이동은 목록 끝에서 멈춘다", () => {
    const r = rows();
    expect(keyAction("ArrowUp", r, 0, new Set())).toEqual({ kind: "focus", index: 0 });
    expect(keyAction("ArrowDown", r, r.length - 1, new Set())).toEqual({
      kind: "focus",
      index: r.length - 1,
    });
  });

  it("오른쪽은 접힌 디렉터리를 펼치고, 이미 펼쳐졌으면 첫 자식으로 내려간다", () => {
    const expanded = new Set(["src"]);
    const r = rows();
    // index 0 = .github (접힘)
    expect(keyAction("ArrowRight", r, 0, expanded)).toEqual({ kind: "expand", path: ".github" });
    // index 1 = src (펼침) → 첫 자식 src/lib
    expect(keyAction("ArrowRight", r, 1, expanded)).toEqual({ kind: "focus", index: 2 });
  });

  it("왼쪽은 펼친 디렉터리를 접고, 그 밖에서는 부모로 올라간다", () => {
    const expanded = new Set(["src"]);
    const r = rows();
    expect(keyAction("ArrowLeft", r, 1, expanded)).toEqual({ kind: "collapse", path: "src" });
    // index 3 = src/App.tsx → 부모 src(index 1)
    expect(keyAction("ArrowLeft", r, 3, expanded)).toEqual({ kind: "focus", index: 1 });
  });

  it("최상위에서 왼쪽은 갈 곳이 없다", () => {
    expect(keyAction("ArrowLeft", rows(), 0, new Set())).toEqual({ kind: "none" });
  });

  it("파일에서 오른쪽은 아무 일도 하지 않는다", () => {
    expect(keyAction("ArrowRight", rows(), 3, new Set(["src"]))).toEqual({ kind: "none" });
  });

  it("Enter는 파일이면 열고 디렉터리면 토글한다", () => {
    const r = rows();
    expect(keyAction("Enter", r, 3, new Set(["src"]))).toEqual({
      kind: "open",
      path: "src/App.tsx",
    });
    expect(keyAction("Enter", r, 1, new Set(["src"]))).toEqual({ kind: "toggle", path: "src" });
  });

  it("Home/End는 목록 양 끝으로 간다", () => {
    const r = rows();
    expect(keyAction("Home", r, 3, new Set())).toEqual({ kind: "focus", index: 0 });
    expect(keyAction("End", r, 0, new Set())).toEqual({ kind: "focus", index: r.length - 1 });
  });

  it("빈 목록에서는 어떤 키도 동작하지 않는다", () => {
    expect(keyAction("ArrowDown", [], -1, new Set())).toEqual({ kind: "none" });
  });
});

describe("typeAheadIndex", () => {
  const rows: NavRow[] = [
    { path: "a", depth: 0, isDir: true, name: "app" },
    { path: "b", depth: 0, isDir: false, name: "Beta.ts" },
    { path: "c", depth: 0, isDir: false, name: "beta2.ts" },
  ];

  it("현재 위치 다음부터 찾고, 끝에 닿으면 앞으로 돌아온다", () => {
    expect(typeAheadIndex(rows, "b", 0)).toBe(1);
    expect(typeAheadIndex(rows, "b", 1)).toBe(2);
    expect(typeAheadIndex(rows, "b", 2)).toBe(1);
  });

  it("대소문자를 가리지 않는다", () => {
    expect(typeAheadIndex(rows, "B", -1)).toBe(1);
  });

  it("없는 글자는 -1", () => {
    expect(typeAheadIndex(rows, "z", 0)).toBe(-1);
  });
});
