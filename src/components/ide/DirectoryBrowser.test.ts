import { describe, expect, it } from "vitest";
import { chainLabel, dirTarget, flattenTree, sortEntries } from "./DirectoryBrowser";
import type { DirEntry } from "../../lib/ipc";

const entry = (over: Partial<DirEntry> & { name: string; path: string }): DirEntry => ({
  is_dir: false,
  size: 0,
  mtime: 0,
  is_repo: false,
  denied: false,
  ...over,
});

const dir = (name: string, path: string, mtime = 0) =>
  entry({ name, path, is_dir: true, mtime });

describe("dirTarget", () => {
  it("디렉터리는 자기 경로를 대상으로 내놓는다", () => {
    expect(dirTarget(dir("novel", "/home/u/study/novel"))).toBe("/home/u/study/novel");
  });

  it("파일은 대상이 아니다 — 폴더 선택이 파일 경로를 집어가지 않게", () => {
    expect(dirTarget(entry({ name: "a.ts", path: "/home/u/a.ts" }))).toBeNull();
  });

  it("읽기 권한 없는 폴더도 대상이 아니다", () => {
    expect(dirTarget(entry({ name: "root", path: "/root", is_dir: true, denied: true }))).toBeNull();
  });
});

describe("sortEntries", () => {
  const entries = [
    entry({ name: "b.txt", path: "/r/b.txt", mtime: 300 }),
    dir("zdir", "/r/zdir", 100),
    entry({ name: "a.txt", path: "/r/a.txt", mtime: 200 }),
  ];

  it("정렬 키와 무관하게 디렉터리를 먼저 둔다", () => {
    expect(sortEntries(entries, "name", false).map((e) => e.name)).toEqual([
      "zdir",
      "a.txt",
      "b.txt",
    ]);
    expect(sortEntries(entries, "mtime", true)[0].name).toBe("zdir");
  });

  it("수정시각 내림차순은 최신 파일이 위로", () => {
    expect(sortEntries(entries, "mtime", true).map((e) => e.name)).toEqual([
      "zdir",
      "b.txt",
      "a.txt",
    ]);
  });
});

describe("flattenTree", () => {
  const children: Record<string, DirEntry[]> = {
    "/r": [dir("src", "/r/src"), entry({ name: "top.txt", path: "/r/top.txt" })],
    "/r/src": [dir("deep", "/r/src/deep"), entry({ name: "a.ts", path: "/r/src/a.ts" })],
    "/r/src/deep": [entry({ name: "d.ts", path: "/r/src/deep/d.ts" })],
  };

  it("접힌 디렉터리의 자식은 나오지 않는다", () => {
    const rows = flattenTree(children, "/r", new Set(), "name", false);
    expect(rows.map((r) => r.entry.name)).toEqual(["src", "top.txt"]);
    expect(rows.every((r) => r.depth === 0)).toBe(true);
  });

  it("펼친 가지만 깊이를 늘려 이어붙인다", () => {
    const rows = flattenTree(children, "/r", new Set(["/r/src"]), "name", false);
    expect(rows.map((r) => [r.entry.name, r.depth])).toEqual([
      ["src", 0],
      ["deep", 1],
      ["a.ts", 1],
      ["top.txt", 0],
    ]);
  });

  it("여러 단계를 펼치면 그만큼 중첩된다", () => {
    const rows = flattenTree(children, "/r", new Set(["/r/src", "/r/src/deep"]), "name", false);
    expect(rows.map((r) => [r.entry.name, r.depth])).toEqual([
      ["src", 0],
      ["deep", 1],
      ["d.ts", 2],
      ["a.ts", 1],
      ["top.txt", 0],
    ]);
  });

  it("자식을 아직 못 받은 디렉터리는 펼쳐도 건너뛴다", () => {
    const rows = flattenTree(children, "/r", new Set(["/r/src", "/r/unknown"]), "name", false);
    expect(rows.map((r) => r.entry.name)).toContain("a.ts");
    expect(rows.some((r) => r.entry.path.startsWith("/r/unknown/"))).toBe(false);
  });

  it("자기 자신을 포함하는 순환도 무한 재귀하지 않는다", () => {
    const looped: Record<string, DirEntry[]> = { "/r": [dir("r", "/r")] };
    const rows = flattenTree(looped, "/r", new Set(["/r"]), "name", false);
    expect(rows).toHaveLength(1);
  });

  it("루트에 자식이 없으면 빈 목록", () => {
    expect(flattenTree({}, "/r", new Set(), "name", false)).toEqual([]);
  });
});

describe("chainLabel", () => {
  it("사슬 이름을 슬래시로 잇는다", () => {
    expect(chainLabel([dir("src", "/r/src"), dir("main", "/r/src/main")])).toBe("src/main");
  });

  it("접히지 않은 행은 이름 그대로다", () => {
    expect(chainLabel([dir("src", "/r/src")])).toBe("src");
  });
});

describe("flattenTree — 단일 자식 압축", () => {
  /** `/r/src/main/java`까지 한 줄로 접힐 수 있는 트리. */
  const chained: Record<string, DirEntry[]> = {
    "/r": [dir("src", "/r/src"), entry({ name: "top.txt", path: "/r/top.txt" })],
    "/r/src": [dir("main", "/r/src/main")],
    "/r/src/main": [dir("java", "/r/src/main/java")],
    "/r/src/main/java": [entry({ name: "A.java", path: "/r/src/main/java/A.java" })],
  };
  const open = new Set(["/r/src", "/r/src/main", "/r/src/main/java"]);

  it("단일 dir 자식 사슬은 한 행으로 합쳐진다", () => {
    const rows = flattenTree(chained, "/r", open, "name", false);
    expect(rows.map((r) => [chainLabel(r.chain), r.depth])).toEqual([
      ["src/main/java", 0],
      ["A.java", 1],
      ["top.txt", 0],
    ]);
    expect(rows[0].chain).toHaveLength(3);
    // 행이 대표하는 것은 사슬의 끝이다 — 선택·이동이 가장 깊은 곳을 가리켜야 한다.
    expect(rows[0].entry.path).toBe("/r/src/main/java");
  });

  it("자식이 둘이면 압축하지 않는다", () => {
    const two = { ...chained, "/r/src": [dir("main", "/r/src/main"), dir("test", "/r/src/test")] };
    const rows = flattenTree(two, "/r", open, "name", false);
    expect(rows.map((r) => [chainLabel(r.chain), r.depth])).toEqual([
      ["src", 0],
      ["main/java", 1],
      ["A.java", 2],
      ["test", 1],
      ["top.txt", 0],
    ]);
  });

  it("유일한 자식이 파일이면 압축하지 않는다", () => {
    const withFile = { ...chained, "/r/src": [entry({ name: "a.ts", path: "/r/src/a.ts" })] };
    const rows = flattenTree(withFile, "/r", open, "name", false);
    expect(rows.map((r) => chainLabel(r.chain))).toEqual(["src", "a.ts", "top.txt"]);
  });

  it("denied 디렉터리에서 사슬이 끊긴다", () => {
    // 권한 없는 폴더는 자식을 받아올 수 없다 — 캐시에도 없는 것이 실제 모양이다.
    const denied: Record<string, DirEntry[]> = {
      "/r": [dir("src", "/r/src"), entry({ name: "top.txt", path: "/r/top.txt" })],
      "/r/src": [dir("main", "/r/src/main")],
      "/r/src/main": [entry({ name: "java", path: "/r/src/main/java", is_dir: true, denied: true })],
    };
    const rows = flattenTree(denied, "/r", open, "name", false);
    expect(rows.map((r) => [chainLabel(r.chain), r.depth])).toEqual([
      ["src/main", 0],
      ["java", 1],
      ["top.txt", 0],
    ]);
  });

  it("사슬 끝이 접혀 있으면 거기까지만 접는다", () => {
    const rows = flattenTree(chained, "/r", new Set(["/r/src"]), "name", false);
    expect(rows.map((r) => chainLabel(r.chain))).toEqual(["src/main", "top.txt"]);
  });

  it("사슬 끝의 자식이 아직 로드되지 않으면 거기까지만 접는다", () => {
    const partial = { "/r": [dir("src", "/r/src")], "/r/src": [dir("main", "/r/src/main")] };
    const rows = flattenTree(partial, "/r", open, "name", false);
    expect(rows.map((r) => chainLabel(r.chain))).toEqual(["src/main"]);
  });

  it("숨김을 뺀 나머지가 단일 dir이면 압축한다", () => {
    const hidden = {
      ...chained,
      "/r/src": [dir("main", "/r/src/main"), entry({ name: ".DS_Store", path: "/r/src/.DS_Store" })],
    };
    expect(
      flattenTree(hidden, "/r", open, "name", false, false).map((r) => chainLabel(r.chain)),
    ).toEqual(["src/main/java", "A.java", "top.txt"]);
    // 숨김을 보이면 자식이 둘이므로 같은 트리라도 접히지 않는다 — 화면과 판정이 어긋나면 안 된다.
    expect(
      flattenTree(hidden, "/r", open, "name", false, true).map((r) => chainLabel(r.chain)),
    ).toContain("src");
  });

  it("사슬이 순환해도 무한 재귀하지 않는다", () => {
    const looped: Record<string, DirEntry[]> = {
      "/r": [dir("a", "/r/a")],
      "/r/a": [dir("b", "/r/a/b")],
      "/r/a/b": [dir("a", "/r/a")],
    };
    const rows = flattenTree(looped, "/r", new Set(["/r/a", "/r/a/b"]), "name", false);
    expect(rows).toHaveLength(1);
  });
});
