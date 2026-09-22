import { describe, expect, it } from "vitest";
import type { FsNode } from "../lib/ipc";
import { breadcrumbs, childrenAt, findNode, parentPath, previewNotice, ROOT } from "./tree";

const dir = (name: string, path: string, children: FsNode[]): FsNode => ({
  name,
  path,
  is_dir: true,
  children,
});
const file = (name: string, path: string): FsNode => ({ name, path, is_dir: false, children: [] });

const tree: FsNode[] = [
  dir("src", "src", [
    dir("lib", "src/lib", [file("diff.ts", "src/lib/diff.ts")]),
    file("main.tsx", "src/main.tsx"),
    file("App.tsx", "src/App.tsx"),
  ]),
  file("README.md", "README.md"),
  dir("srcs", "srcs", [file("decoy.txt", "srcs/decoy.txt")]),
];

describe("findNode", () => {
  it("중첩 경로를 찾는다", () => {
    expect(findNode(tree, "src/lib/diff.ts")?.name).toBe("diff.ts");
    expect(findNode(tree, "src/lib")?.is_dir).toBe(true);
    expect(findNode(tree, "README.md")?.name).toBe("README.md");
  });

  it("접두사만 같은 형제로 새지 않는다", () => {
    // "src"와 "srcs"는 접두사가 겹친다 — 경계를 안 보면 엉뚱한 트리를 뒤진다.
    expect(findNode(tree, "srcs/decoy.txt")?.name).toBe("decoy.txt");
    expect(findNode(tree, "src/decoy.txt")).toBeNull();
  });

  it("루트와 없는 경로는 null", () => {
    expect(findNode(tree, ROOT)).toBeNull();
    expect(findNode(tree, "nope")).toBeNull();
    expect(findNode(tree, "src/nope/deep.ts")).toBeNull();
  });
});

describe("childrenAt", () => {
  it("루트에서는 최상위를 보여준다", () => {
    expect(childrenAt(tree, ROOT).map((n) => n.name)).toEqual(["src", "srcs", "README.md"]);
  });

  it("디렉터리 먼저, 그다음 이름순", () => {
    expect(childrenAt(tree, "src").map((n) => n.name)).toEqual(["lib", "App.tsx", "main.tsx"]);
  });

  it("파일이나 없는 경로면 빈 목록", () => {
    expect(childrenAt(tree, "README.md")).toEqual([]);
    expect(childrenAt(tree, "nope")).toEqual([]);
  });
});

describe("parentPath", () => {
  it("한 단계 위로 간다", () => {
    expect(parentPath("src/lib/diff.ts")).toBe("src/lib");
    expect(parentPath("src")).toBe(ROOT);
    expect(parentPath(ROOT)).toBe(ROOT);
  });
});

describe("breadcrumbs", () => {
  it("루트부터 현재까지 누적 경로를 만든다", () => {
    expect(breadcrumbs("src/lib/diff.ts")).toEqual([
      { name: "/", path: "" },
      { name: "src", path: "src" },
      { name: "lib", path: "src/lib" },
      { name: "diff.ts", path: "src/lib/diff.ts" },
    ]);
  });

  it("루트만 있을 때도 항목이 하나 있다", () => {
    expect(breadcrumbs(ROOT)).toEqual([{ name: "/", path: "" }]);
  });
});

describe("previewNotice", () => {
  it("열 수 없는 이유를 말한다", () => {
    // 빈 화면으로 두면 앱이 고장난 것으로 보인다.
    expect(previewNotice("binary")).toContain("텍스트가 아니");
    expect(previewNotice("too_large")).toContain("너무 커서");
  });

  it("볼 수 있는 종류에는 안내를 붙이지 않는다", () => {
    expect(previewNotice("text")).toBeNull();
    expect(previewNotice("image")).toBeNull();
  });
});
