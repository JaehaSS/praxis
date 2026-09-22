import { describe, expect, it } from "vitest";
import { FILE_GLYPHS } from "./file-glyphs";
import { fileGlyph, fileIcon, iconTint } from "./file-icons";

describe("fileGlyph", () => {
  it("언어를 확장자로 가른다", () => {
    expect(fileGlyph("App.tsx")).toBe("react");
    expect(fileGlyph("main.rs")).toBe("rust");
    expect(fileGlyph("a.py")).toBe("python");
    expect(fileGlyph("Cargo.toml")).toBe("toml");
  });

  it("파일명 전체 일치가 확장자보다 우선한다 (fileIcon과 같은 순서)", () => {
    // 두 함수가 다른 순서를 쓰면 같은 파일이 아이콘과 틴트에서 다르게 분류된다.
    expect(fileGlyph("package.json")).toBe("npm");
    expect(fileGlyph("other.json")).toBe("json");
    expect(fileGlyph("Cargo.lock")).toBe("lock");
  });

  it("테스트 파일을 따로 표시한다 (가장 긴 접미가 이긴다)", () => {
    expect(fileGlyph("a.test.ts")).toBe("test-ts");
    expect(fileGlyph("a.spec.js")).toBe("test-js");
    // `.behavior.test.tsx`가 `.test.tsx`보다 먼저 잡혀야 한다 — 둘 다 매치하므로 길이가 가른다.
    expect(fileGlyph("FileTree.behavior.test.tsx")).toBe("test-ts");
    // 접미가 아니면 확장자로 떨어진다.
    expect(fileGlyph("testing.ts")).toBe("typescript");
  });

  it("모든 글리프가 어딘가에서 도달 가능하다", () => {
    // 도달 불가 글리프는 그냥 번들 무게다. 매핑을 지우면서 추출 목록을 안 지우면 쌓인다.
    const names = [
      "App.tsx", "a.ts", "a.js", "m.rs", "a.py", "x.go", "A.java", "a.kt", "a.swift",
      "a.rb", "a.php", "a.c", "a.cpp", "a.cs", "i.html", "s.css", "s.scss",
      "d.json", "d.yaml", "Cargo.toml", "d.xml", "R.md", "q.sql", "r.sh",
      "p.png", "f.woff2", "n.txt", "Dockerfile", ".gitignore", "Cargo.lock",
      "package.json", "tailwind.config.ts", "vite.config.ts", "tsconfig.json",
      "a.test.ts", "a.test.js",
    ];
    const reached = new Set<string>(names.map(fileGlyph).filter((g): g is NonNullable<typeof g> => g !== null));
    const all = Object.keys(FILE_GLYPHS);
    expect(all.filter((n) => !reached.has(n))).toEqual([]);
  });

  it("모르는 종류는 null — 호출부가 단색으로 후퇴한다", () => {
    expect(fileGlyph("CHANGELOG")).toBeNull();
    expect(fileGlyph("weird.qqq")).toBeNull();
    expect(fileGlyph(".hidden")).toBeNull();
  });

  it("가리키는 글리프가 실제로 존재한다", () => {
    // 오타로 없는 이름을 가리키면 런타임에 빈 아이콘이 된다.
    for (const name of ["App.tsx", "main.rs", "a.py", "x.go", "s.scss", "d.sql", "Dockerfile"]) {
      const g = fileGlyph(name);
      if (g) expect(FILE_GLYPHS[g], name).toBeTruthy();
    }
  });

  it("모든 글리프가 경로를 하나 이상 갖는다", () => {
    for (const [name, g] of Object.entries(FILE_GLYPHS)) {
      expect(g.paths.length, name).toBeGreaterThan(0);
      expect(g.viewBox, name).toMatch(/^[\d.\s-]+$/);
    }
  });

  it("후퇴 경로가 여전히 틴트를 받는다", () => {
    // 글리프가 없는 종류에서 "형태 + 저채도 틴트" 계약이 살아 있어야 한다.
    expect(fileGlyph("notes.txt")).toBe("document");
    const noGlyph = "CHANGELOG";
    expect(fileGlyph(noGlyph)).toBeNull();
    expect(iconTint(fileIcon("a.sh"))).toBeTruthy();
  });
});
