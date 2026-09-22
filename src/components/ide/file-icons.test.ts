import { describe, expect, it } from "vitest";
import type { IconName } from "./icons";
import { fileIcon, folderIcon, iconTint, isHiddenName } from "./file-icons";

/** index.css(`--ft-*`)가 실제로 정의하는 틴트 전부. 여기 없는 클래스는 화면에서 조용히 무색이 된다. */
const PALETTE = [
  "text-ft-folder",
  "text-ft-code",
  "text-ft-doc",
  "text-ft-style",
  "text-ft-data",
] as const;

/** `fileIcon()`이 낼 수 있는 아이콘 전부(BY_NAME ∪ BY_EXT ∪ 폴백). 두 맵은 export되지 않으므로
 *  여기 손으로 적어 둔다 — 맵에 새 아이콘을 넣고 이 목록을 안 고치면 망라 검사가 그것을 놓친다.
 *  대신 새 아이콘에 틴트를 빠뜨리면 아래 검사가 깨진다. */
const FILE_ICONS: IconName[] = [
  "fileCode",
  "terminal",
  "database",
  "braces",
  "lock",
  "branch",
  "fileText",
  "markdown",
  "chart",
  "code",
  "image",
  "palette",
  "file",
];

/** 일부러 무색으로 두는 아이콘 — 카테고리가 아니라 "표시할 것이 없음"을 뜻한다. */
const COLORLESS: IconName[] = ["lock", "branch", "file"];

/** 트리가 실제로 그리는 아이콘 전부 = 파일 아이콘 + 디렉터리 두 종류. */
const TREE_ICONS: IconName[] = [...FILE_ICONS, folderIcon(true), folderIcon(false)];

describe("fileIcon", () => {
  it("확장자로 종류를 가른다", () => {
    expect(fileIcon("App.tsx")).toBe("fileCode");
    expect(fileIcon("mod.rs")).toBe("fileCode");
    expect(fileIcon("tsconfig.json")).toBe("braces");
    expect(fileIcon("README.md")).toBe("markdown");
    expect(fileIcon("logo.png")).toBe("image");
    expect(fileIcon("index.css")).toBe("palette");
    expect(fileIcon("build.sh")).toBe("terminal");
  });

  it("대소문자를 가리지 않는다", () => {
    expect(fileIcon("README.MD")).toBe("markdown");
    expect(fileIcon("Main.PY")).toBe("fileCode");
  });

  it("이름 규칙이 확장자보다 우선한다", () => {
    // Cargo.lock은 확장자 규칙으로도 lock이지만, Dockerfile은 점이 아예 없다.
    expect(fileIcon("Dockerfile")).toBe("database");
    expect(fileIcon("Makefile")).toBe("terminal");
    expect(fileIcon("package-lock.json")).toBe("lock");
    expect(fileIcon(".gitignore")).toBe("branch");
  });

  it("점이 여럿이면 마지막 조각이 종류를 정한다", () => {
    expect(fileIcon("vite.config.ts")).toBe("fileCode");
    expect(fileIcon("docker-compose.yml")).toBe("braces");
  });

  it("모르는 종류와 확장자 없는 이름은 무지 파일", () => {
    expect(fileIcon("CHANGELOG")).toBe("file");
    expect(fileIcon("mystery.qqq")).toBe("file");
  });
});

describe("folderIcon", () => {
  it("펼침 여부로 실루엣이 갈린다", () => {
    expect(folderIcon(true)).toBe("folderOpen");
    expect(folderIcon(false)).toBe("folder");
  });
});

describe("iconTint", () => {
  it("아이콘 실루엣이 곧 틴트의 카테고리다", () => {
    expect(iconTint("folder")).toBe("text-ft-folder");
    expect(iconTint("folderOpen")).toBe("text-ft-folder");
    expect(iconTint("fileCode")).toBe("text-ft-code");
    expect(iconTint("terminal")).toBe("text-ft-code");
    expect(iconTint("markdown")).toBe("text-ft-doc");
    expect(iconTint("palette")).toBe("text-ft-style");
    expect(iconTint("braces")).toBe("text-ft-data");
  });

  it("문서 계열은 실루엣이 달라도 한 틴트로 묶인다", () => {
    // 본문·표는 읽을거리라는 점에서 같다 — markdown과 색이 갈리면 카테고리가 셋으로 보인다.
    expect(iconTint("fileText")).toBe("text-ft-doc");
    expect(iconTint("chart")).toBe("text-ft-doc");
  });

  it("마크업과 이미지는 스타일 계열로 묶인다", () => {
    // html/xml은 코드처럼 생겼지만 코드 틴트를 주면 .ts와 구분이 사라진다.
    expect(iconTint("image")).toBe("text-ft-style");
    expect(iconTint("code")).toBe("text-ft-style");
  });

  it("데이터·바이너리는 데이터 틴트를 쓴다", () => {
    expect(iconTint("database")).toBe("text-ft-data");
  });

  it("규칙이 없는 아이콘은 무색으로 둔다", () => {
    // 호출부가 muted로 떨어뜨린다 — 여기서 회색을 정하면 활성 행 색까지 덮는다.
    expect(iconTint("lock")).toBeUndefined();
    expect(iconTint("file")).toBeUndefined();
    expect(iconTint("branch")).toBeUndefined();
  });
});

describe("iconTint — 망라", () => {
  it("트리가 그리는 모든 아이콘이 색을 받거나, 명시적으로 무색이다", () => {
    // 새 아이콘을 틴트 없이 추가하면 여기서 걸린다 — 화면에서는 회색 하나가 늘 뿐이라 안 보인다.
    const unexpected = FILE_ICONS.filter(
      (icon) => iconTint(icon) === undefined && !COLORLESS.includes(icon),
    );
    expect(unexpected).toEqual([]);
  });

  it("무색 목록은 정말로 무색이다", () => {
    // 반대 방향 — 무색이어야 할 것에 색이 붙으면 lock/branch가 카테고리인 척하게 된다.
    expect(COLORLESS.filter((icon) => iconTint(icon) !== undefined)).toEqual([]);
  });

  it("틴트 값은 팔레트 밖으로 나가지 않는다", () => {
    // 오타(text-ft-styles)는 타입이 string이라 통과하고, 화면에서 색만 조용히 빠진다.
    const outside = TREE_ICONS.map(iconTint).filter(
      (tint): tint is string => tint !== undefined && !PALETTE.includes(tint as never),
    );
    expect(outside).toEqual([]);
  });

  it("팔레트의 다섯 색이 모두 쓰인다", () => {
    // 한 색이 아무 아이콘에도 안 붙으면 그 카테고리가 화면에서 사라진 것이다.
    const used = new Set(TREE_ICONS.map(iconTint).filter(Boolean));
    expect([...PALETTE].filter((tint) => !used.has(tint))).toEqual([]);
  });
});

describe("isHiddenName", () => {
  it("점으로 시작하면 숨김", () => {
    expect(isHiddenName(".github")).toBe(true);
    expect(isHiddenName("src")).toBe(false);
  });
});
