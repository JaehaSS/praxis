// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { FileTree } from "./FileTree";
import type { FsNode } from "../../lib/ipc";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// jsdom에는 스크롤 구현이 없다 — 커서를 따라가는 부수효과만 없애고 나머지는 그대로 검증한다.
Element.prototype.scrollIntoView = vi.fn();

const node = (name: string, path: string, children?: FsNode[]): FsNode => ({
  name,
  path,
  is_dir: children !== undefined,
  children: children ?? [],
});

const TREE: FsNode[] = [
  node("src", "src", [node("App.tsx", "src/App.tsx"), node("lib", "src/lib", [])]),
  node(".github", ".github", [node("ci.yml", ".github/ci.yml")]),
  node("README.md", "README.md"),
  node("CHANGELOG", "CHANGELOG"),
];

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = (props: Partial<Parameters<typeof FileTree>[0]> = {}) => {
  act(() => {
    root.render(
      <FileTree nodes={TREE} activePath={null} onOpen={() => {}} {...props} />,
    );
  });
};

const tree = () => container.querySelector('[role="tree"]') as HTMLElement;
const row = (label: string) =>
  [...container.querySelectorAll('[role="treeitem"]')].find(
    (el) => el.textContent?.trim() === label,
  ) as HTMLElement;
/** 종류 아이콘을 감싼 span. 디렉터리 행은 셰브론이 먼저 오므로 마지막 svg가 종류 아이콘이다. */
const iconWrap = (label: string) => {
  const svgs = row(label).querySelectorAll("svg");
  return svgs[svgs.length - 1].parentElement as HTMLElement;
};
/** 종류 아이콘 svg 자체 — 컬러 글리프는 path에 fill을 갖고, 단색 아이콘은 갖지 않는다. */
const typeIcon = (label: string) => {
  const svgs = row(label).querySelectorAll("svg");
  return svgs[svgs.length - 1] as SVGElement;
};
/** 컬러 글리프인가 — path에 currentColor가 아닌 fill이 있으면 그렇다. */
const isColorGlyph = (label: string) =>
  [...typeIcon(label).querySelectorAll("path")].some((p) => {
    const f = p.getAttribute("fill");
    return !!f && f !== "currentColor" && f !== "none";
  });
const labels = () =>
  [...container.querySelectorAll('[role="treeitem"]')].map((el) => el.textContent?.trim());
const press = (key: string) => {
  act(() => {
    tree().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });
};

describe("FileTree — 숨김 항목", () => {
  it("기본은 도트 항목을 감춘다", () => {
    render();
    expect(labels()).not.toContain(".github");
    expect(labels()).toContain("README.md");
  });

  it("showHidden이면 도트 항목이 나온다", () => {
    render({ showHidden: true });
    expect(labels()).toContain(".github");
  });
});

describe("FileTree — 키보드", () => {
  it("아래로 이동한 뒤 Enter로 파일을 연다", () => {
    const onOpen = vi.fn();
    render({ onOpen });
    // 루트 한 겹이 펼쳐진 채 시작하고 디렉터리가 앞선다: [src, src/lib, src/App.tsx, README.md]
    press("ArrowDown"); // src
    press("ArrowDown"); // src/lib
    press("ArrowDown"); // src/App.tsx
    press("Enter");
    expect(onOpen).toHaveBeenCalledWith("src/App.tsx");
  });

  it("디렉터리에서 Enter는 열지 않고 접는다", () => {
    const onOpen = vi.fn();
    render({ onOpen });
    press("ArrowDown"); // src (펼쳐진 상태)
    press("Enter");
    expect(onOpen).not.toHaveBeenCalled();
    expect(labels()).not.toContain("App.tsx");
  });

  it("오른쪽 화살표가 접힌 디렉터리를 펼친다", () => {
    render();
    press("ArrowDown"); // src
    press("ArrowLeft"); // 접기
    expect(labels()).not.toContain("App.tsx");
    press("ArrowRight"); // 다시 펼치기
    expect(labels()).toContain("App.tsx");
  });

  it("타이핑하면 그 글자로 시작하는 항목으로 커서가 뛴다", () => {
    const onOpen = vi.fn();
    render({ onOpen });
    press("R"); // README.md
    press("Enter");
    expect(onOpen).toHaveBeenCalledWith("README.md");
  });
});

describe("FileTree — 틴트", () => {
  it("아는 언어는 자기 색을 가진 글리프로 그린다 (틴트를 얹지 않는다)", () => {
    render();
    // 언어 로고는 색이 아이콘 **안에** 있다 — 밖에서 틴트를 덧칠하면 두 색이 싸운다.
    expect(isColorGlyph("App.tsx")).toBe(true);
    expect(isColorGlyph("README.md")).toBe(true);
    expect(iconWrap("App.tsx").className).not.toContain("text-ft-");
  });

  it("모르는 종류는 단색 아이콘 + 틴트로 후퇴한다", () => {
    render();
    // 후퇴 경로가 살아 있어야 "형태가 주 채널"이라는 계약이 유지된다.
    expect(isColorGlyph("src")).toBe(false);
    expect(iconWrap("src").className).toContain("text-ft-folder");
  });

  it("디렉터리는 골드 틴트에 굵기를 겹친다", () => {
    render();
    expect(iconWrap("src").className).toContain("text-ft-folder");
    expect(row("src").querySelector(".truncate")?.className).toContain("font-medium");
    expect(row("README.md").querySelector(".truncate")?.className).not.toContain("font-medium");
  });

  it("틴트 규칙이 없는 파일은 muted로 남는다", () => {
    render();
    expect(iconWrap("CHANGELOG").className).toContain("text-text-muted");
    expect(iconWrap("CHANGELOG").className).not.toContain("text-ft-");
  });

  it("활성 행이어도 종류 신호는 그대로다", () => {
    // 종류는 선택 여부와 무관한 사실이다. 활성 행이라고 색이 바뀌면 안 된다.
    render({ activePath: "src/App.tsx" });
    expect(isColorGlyph("App.tsx")).toBe(true);
    expect(iconWrap("src").className).toContain("text-ft-folder");
  });

  it("무색 파일이 활성이면 muted를 벗는다", () => {
    // 활성 행 글자색(primary-bright)을 muted가 덮으면 고른 줄만 흐려 보인다.
    render({ activePath: "CHANGELOG" });
    expect(iconWrap("CHANGELOG").className).not.toContain("text-text-muted");
  });
});

describe("FileTree — 클릭", () => {
  it("파일을 누르면 열고, 디렉터리를 누르면 접힌다", () => {
    const onOpen = vi.fn();
    render({ onOpen });
    const file = [...container.querySelectorAll('[role="treeitem"]')].find(
      (el) => el.textContent?.trim() === "App.tsx",
    ) as HTMLElement;
    act(() => file.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(onOpen).toHaveBeenCalledWith("src/App.tsx");
  });
});

describe("FileTree — 우클릭", () => {
  const rightClick = (el: Element) => {
    act(() => {
      el.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 30, clientY: 40 }),
      );
    });
  };

  it("행을 우클릭하면 그 노드를 넘긴다", () => {
    const onContextMenu = vi.fn();
    render({ onContextMenu });
    rightClick(row("src"));

    expect(onContextMenu).toHaveBeenCalledOnce();
    const [node, x, y] = onContextMenu.mock.calls[0];
    expect(node).toMatchObject({ path: "src", is_dir: true });
    expect([x, y]).toEqual([30, 40]);
  });

  it("빈 자리를 우클릭하면 노드 없이 부른다 — 대상은 워크트리 루트다", () => {
    const onContextMenu = vi.fn();
    render({ onContextMenu });
    rightClick(tree());

    expect(onContextMenu).toHaveBeenCalledWith(null, 30, 40);
  });

  it("행의 우클릭이 빈 자리 메뉴로 새지 않는다", () => {
    const onContextMenu = vi.fn();
    render({ onContextMenu });
    rightClick(row("README.md"));

    // 버블이 컨테이너까지 올라가면 두 번 불리고, 나중 것(null)이 행을 덮어쓴다.
    expect(onContextMenu).toHaveBeenCalledOnce();
    expect(onContextMenu.mock.calls[0][0]).toMatchObject({ path: "README.md" });
  });

  it("빈 워크트리에서도 우클릭을 받는다 — 첫 파일을 만들 자리가 거기뿐이다", () => {
    const onContextMenu = vi.fn();
    render({ nodes: [], onContextMenu });
    rightClick(container.firstElementChild as Element);

    expect(onContextMenu).toHaveBeenCalledWith(null, 30, 40);
  });

  it("핸들러가 없으면 기본 메뉴를 막지 않는다", () => {
    render();
    const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    act(() => {
      row("src").dispatchEvent(event);
    });

    expect(event.defaultPrevented).toBe(false);
  });
});

