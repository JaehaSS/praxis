import { describe, expect, it } from "vitest";
import type { FsNode } from "../../lib/ipc";
import {
  affectedPaths,
  joinPath,
  namesIn,
  renamedPath,
  rewritePath,
  targetDir,
} from "./tree-file-ops";

const dir = (name: string, path: string, children: FsNode[] = []): FsNode => ({
  name,
  path,
  is_dir: true,
  children,
});
const file = (name: string, path: string): FsNode => ({ name, path, is_dir: false, children: [] });

const TREE: FsNode[] = [
  dir("src", "src", [
    dir("lib", "src/lib", [file("ipc.ts", "src/lib/ipc.ts")]),
    file("App.tsx", "src/App.tsx"),
  ]),
  file("README.md", "README.md"),
];

describe("새 항목이 생길 자리", () => {
  it("디렉터리를 짚으면 그 안이다", () => {
    expect(targetDir(dir("lib", "src/lib"))).toBe("src/lib");
  });

  it("파일을 짚으면 그 옆이다 — 파일 안에는 만들 수 없다", () => {
    expect(targetDir(file("ipc.ts", "src/lib/ipc.ts"))).toBe("src/lib");
  });

  it("최상위 파일을 짚으면 루트다", () => {
    expect(targetDir(file("README.md", "README.md"))).toBe("");
  });

  it("아무것도 짚지 않았으면 루트다", () => {
    expect(targetDir(null)).toBe("");
  });
});

describe("경로 잇기", () => {
  it("루트에 붙일 때 앞 슬래시를 남기지 않는다", () => {
    expect(joinPath("", "새.ts")).toBe("새.ts");
  });

  it("하위 디렉터리에는 슬래시로 잇는다", () => {
    expect(joinPath("src/lib", "새.ts")).toBe("src/lib/새.ts");
  });
});

describe("그 디렉터리의 이름들", () => {
  it("루트는 최상위 이름들이다", () => {
    expect([...namesIn(TREE, "")].sort()).toEqual(["README.md", "src"]);
  });

  it("깊은 디렉터리도 따라 내려간다", () => {
    expect([...namesIn(TREE, "src/lib")]).toEqual(["ipc.ts"]);
  });

  it("없는 경로면 빈 집합 — 최종 판정은 백엔드가 한다", () => {
    expect(namesIn(TREE, "src/없다").size).toBe(0);
  });

  it("같은 이름의 파일을 디렉터리로 착각하지 않는다", () => {
    // "src/App.tsx"는 파일이므로 그 아래로 내려갈 수 없다.
    expect(namesIn(TREE, "src/App.tsx").size).toBe(0);
  });
});

describe("이름이 바뀐 자리 따라가기", () => {
  it("대상 자신은 새 경로가 된다", () => {
    expect(rewritePath("src/a.ts", "src/a.ts", "src/b.ts")).toBe("src/b.ts");
  });

  it("디렉터리 아래 것들도 함께 따라간다", () => {
    expect(rewritePath("src/lib/ipc.ts", "src/lib", "src/core")).toBe("src/core/ipc.ts");
  });

  it("이름이 앞부분만 겹치는 것은 건드리지 않는다", () => {
    // "src/lib2"는 "src/lib"의 하위가 아니다 — 슬래시 경계를 봐야 한다.
    expect(rewritePath("src/lib2/x.ts", "src/lib", "src/core")).toBeNull();
  });

  it("무관한 경로는 null", () => {
    expect(rewritePath("docs/README.md", "src", "app")).toBeNull();
  });
});

describe("영향받는 탭", () => {
  const OPEN = ["src/a.ts", "src/lib/ipc.ts", "src/lib2/x.ts", "README.md"];

  it("파일을 건드리면 그 파일뿐이다", () => {
    expect(affectedPaths(OPEN, "src/a.ts")).toEqual(["src/a.ts"]);
  });

  it("디렉터리를 건드리면 그 아래 전부다", () => {
    expect(affectedPaths(OPEN, "src")).toEqual(["src/a.ts", "src/lib/ipc.ts", "src/lib2/x.ts"]);
  });

  it("앞부분만 겹치는 형제는 빠진다", () => {
    expect(affectedPaths(OPEN, "src/lib")).toEqual(["src/lib/ipc.ts"]);
  });

  it("열린 것이 없으면 빈 목록", () => {
    expect(affectedPaths([], "src")).toEqual([]);
  });
});

describe("이름 바꾼 뒤의 경로", () => {
  it("부모는 그대로 두고 마지막 마디만 바꾼다", () => {
    expect(renamedPath("src/lib/ipc.ts", "transport.ts")).toBe("src/lib/transport.ts");
  });

  it("최상위 항목도 다룬다", () => {
    expect(renamedPath("README.md", "READ.md")).toBe("READ.md");
  });
});

