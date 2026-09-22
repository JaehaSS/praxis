// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  LinkContextMenu,
  type LinkMenuAction,
  type LinkMenuKind,
  type LinkMenuState,
} from "./LinkContextMenu";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const MENU: LinkMenuState = { x: 10, y: 10, link: "src/lib/ipc.ts" };

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

interface RenderOptions {
  menu?: LinkMenuState | null;
  kind?: LinkMenuKind;
  supportsExternalPath?: boolean;
  onAction?: (action: LinkMenuAction) => void;
}

const render = ({
  menu = MENU,
  kind = "file",
  supportsExternalPath = true,
  onAction = () => {},
}: RenderOptions = {}) => {
  act(() => {
    root.render(
      <LinkContextMenu
        menu={menu}
        kind={kind}
        supportsExternalPath={supportsExternalPath}
        onAction={onAction}
        onClose={() => {}}
      />,
    );
  });
};

const items = () => Array.from(container.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'));
const labels = () => items().map((b) => b.textContent);

describe("LinkContextMenu", () => {
  it("메뉴가 없으면 아무것도 그리지 않는다", () => {
    render({ menu: null });
    expect(container.querySelector('[role="menu"]')).toBeNull();
  });

  it("로컬 파일 링크는 열기·복사·외부 앱까지 모두 낸다", () => {
    render();
    expect(labels()).toEqual([
      "열기",
      "링크 텍스트 복사",
      "절대 경로 복사",
      "Finder에서 보기",
      "기본 앱으로 열기",
    ]);
  });

  it("원격이면 실경로에 기대는 항목이 통째로 없다", () => {
    render({ supportsExternalPath: false });
    const shown = labels();
    for (const gone of ["절대 경로 복사", "Finder에서 보기", "기본 앱으로 열기"]) {
      expect(shown).not.toContain(gone);
    }
    // 열기와 원문 복사는 남는다 — 둘 다 클라이언트 OS 경로를 필요로 하지 않는다.
    expect(shown).toEqual(["열기", "링크 텍스트 복사"]);
  });

  it("url은 브라우저 열기와 복사 둘뿐이다", () => {
    render({ kind: "url", menu: { x: 0, y: 0, link: "https://example.com/a" } });
    expect(labels()).toEqual(["브라우저에서 열기", "링크 복사"]);
  });

  it("해석하지 못한 링크는 복사 하나만 준다", () => {
    render({ kind: "unresolved", menu: { x: 0, y: 0, link: "어딘가/없는파일.ts" } });
    expect(labels()).toEqual(["링크 텍스트 복사"]);
  });

  it("항목을 누르면 그 액션 키로 onAction이 불린다", () => {
    const onAction = vi.fn();
    render({ onAction });
    const reveal = items().find((b) => b.textContent === "Finder에서 보기")!;
    act(() => reveal.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(onAction).toHaveBeenCalledWith("reveal");
  });

  it("머리말은 파일이면 파일명, url이면 원문을 그대로 보인다", () => {
    render();
    expect(container.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe(
      "ipc.ts 링크 조작",
    );
    render({ kind: "url", menu: { x: 0, y: 0, link: "https://example.com/a/" } });
    expect(container.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe(
      "https://example.com/a/ 링크 조작",
    );
  });
});
