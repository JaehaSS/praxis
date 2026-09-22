import { describe, expect, it } from "vitest";
import { actionDir, copyName, mutationBlock, parentPath, validateName } from "./file-ops";
import type { Task } from "./ipc";

const task = (id: number, worktree_path: string, state = "Running") =>
  ({ id, worktree_path, state }) as Task;

describe("copyName", () => {
  it("확장자 앞에 사본을 붙인다", () => {
    expect(copyName("a.txt", new Set())).toBe("a.txt");
    expect(copyName("a.txt", new Set(["a.txt"]))).toBe("a 사본.txt");
  });

  it("복합 확장자를 하나로 본다", () => {
    expect(copyName("a.tar.gz", new Set(["a.tar.gz"]))).toBe("a 사본.tar.gz");
  });

  it("도트 파일의 앞점은 확장자가 아니다", () => {
    expect(copyName(".env", new Set([".env"]))).toBe(".env 사본");
  });

  it("이미 사본이 있으면 번호를 올린다", () => {
    expect(copyName("a.txt", new Set(["a.txt", "a 사본.txt"]))).toBe("a 사본 2.txt");
  });

  it("확장자 없는 이름도 처리한다", () => {
    expect(copyName("README", new Set(["README"]))).toBe("README 사본");
  });
});

describe("mutationBlock", () => {
  it("워크트리 하위면 막는다", () => {
    expect(mutationBlock("/w/proj/src/a.ts", [task(7, "/w/proj")])?.taskId).toBe(7);
  });

  it("경계에서 끊는다 — /a/work가 /a/workspace를 삼키지 않는다", () => {
    expect(mutationBlock("/a/workspace/x.ts", [task(1, "/a/work")])).toBeNull();
  });

  it("워크트리 자기 자신도 막는다", () => {
    expect(mutationBlock("/w/proj", [task(7, "/w/proj")])?.taskId).toBe(7);
  });

  it("소유 작업이 없으면 통과", () => {
    expect(mutationBlock("/tmp/log.txt", [task(1, "/w/proj")])).toBeNull();
  });

  it("사유에 작업 번호가 들어간다", () => {
    expect(mutationBlock("/w/proj/a.ts", [task(12, "/w/proj")])?.reason).toContain("#12");
  });

  it("여러 작업 중 소유한 것을 찾는다", () => {
    const tasks = [task(1, "/a"), task(2, "/b"), task(3, "/c")];
    expect(mutationBlock("/b/deep/x.ts", tasks)?.taskId).toBe(2);
  });
});

describe("parentPath", () => {
  it("한 단계 위를 돌려준다", () => {
    expect(parentPath("/a/b/c.txt")).toBe("/a/b");
    expect(parentPath("/a/b")).toBe("/a");
  });

  it("최상위 바로 아래는 루트", () => {
    expect(parentPath("/a.txt")).toBe("/");
  });

  it("구분자가 없으면 루트", () => {
    expect(parentPath("a.txt")).toBe("/");
  });

  it("폴더 경로에도 같은 규칙 — 자기 자신을 돌려주지 않는다", () => {
    // 휴지통·이름 변경이 이 성질에 기댄다. 자기 자신이면 지워진 경로를 다시 읽게 된다.
    expect(parentPath("/w/proj/sub")).not.toBe("/w/proj/sub");
    expect(parentPath("/w/proj/sub")).toBe("/w/proj");
  });
});

describe("actionDir", () => {
  it("생성·붙여넣기는 폴더 안에 넣는다", () => {
    for (const action of ["newFile", "newDir", "paste"] as const) {
      expect(actionDir(action, "/w/proj/sub", true)).toBe("/w/proj/sub");
    }
  });

  it("생성·붙여넣기를 파일에 걸면 그 형제 자리다", () => {
    for (const action of ["newFile", "newDir", "paste"] as const) {
      expect(actionDir(action, "/w/proj/a.txt", false)).toBe("/w/proj");
    }
  });

  it("이름 변경·휴지통·중복은 폴더여도 부모다", () => {
    // 폴더에서만 갈리는 지점 — 자기 자신을 쓰면 지워진 경로를 다시 읽고(휴지통),
    // 자식과 이름을 비교하며(이름 변경), 자기 안으로 복사한다(중복).
    for (const action of ["rename", "trash", "duplicate"] as const) {
      expect(actionDir(action, "/w/proj/sub", true)).toBe("/w/proj");
      expect(actionDir(action, "/w/proj/a.txt", false)).toBe("/w/proj");
    }
  });

  it("최상위 폴더를 지워도 루트로 떨어진다", () => {
    expect(actionDir("trash", "/proj", true)).toBe("/");
  });
});

describe("validateName", () => {
  it.each([
    ["", "비어"],
    ["a/b", "/"],
    ["..", ".."],
    [".", ".."],
  ])("%s 를 거부한다", (name, fragment) => {
    expect(validateName(name)).toContain(fragment);
  });

  it("정상 이름은 null", () => {
    expect(validateName("a.txt")).toBeNull();
  });

  it("이미 있는 이름은 중복으로 막는다", () => {
    expect(validateName("a.txt", new Set(["a.txt"]))).toContain("이미");
  });

  it("앞뒤 공백은 잘라내고 판정한다", () => {
    expect(validateName("  a.txt  ", new Set(["a.txt"]))).toContain("이미");
  });
});
