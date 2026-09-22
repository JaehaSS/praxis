// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { FileContextMenu, type FileMenuAction, type FileMenuState } from "./FileContextMenu";
import type { DirEntry } from "../../lib/ipc";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const entry = (name: string, path: string, is_dir = false): DirEntry =>
  ({ name, path, is_dir, size: 0, mtime: 0 }) as DirEntry;

const MENU: FileMenuState = { x: 10, y: 10, entry: entry("a.txt", "/w/a.txt") };

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
  block?: { taskId: number; reason: string } | null;
  readOnly?: boolean;
  clipboard?: string | null;
  onAction?: (action: FileMenuAction) => void;
}

const render = ({
  block = null,
  readOnly = false,
  clipboard = null,
  onAction = () => {},
}: RenderOptions = {}) => {
  act(() => {
    root.render(
      <FileContextMenu
        menu={MENU}
        block={block}
        readOnly={readOnly}
        clipboard={clipboard}
        onAction={onAction}
        onClose={() => {}}
      />,
    );
  });
};

const items = () => Array.from(container.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'));
const labels = () => items().map((b) => b.textContent);

describe("FileContextMenu", () => {
  it("읽기 계열과 변경 계열을 모두 낸다", () => {
    render();
    expect(labels()).toContain("터미널에서 열기");
    expect(labels()).toContain("휴지통으로 이동");
    expect(labels()).toContain("경로 복사");
  });

  it("클립보드가 비면 붙여넣기 항목 자체를 숨긴다 — 비활성이 아니라 제거", () => {
    render({ clipboard: null });
    expect(labels()).not.toContain("붙여넣기");
    render({ clipboard: "/w/b.txt" });
    expect(labels()).toContain("붙여넣기");
  });

  it("원격이면 변경·복제 그룹이 통째로 없다", () => {
    render({ readOnly: true });
    const shown = labels();
    for (const gone of ["복사", "중복", "이름 변경…", "휴지통으로 이동", "새 파일…", "새 폴더…"]) {
      expect(shown).not.toContain(gone);
    }
    // 읽기 계열은 남는다 — 원격에서도 열고 경로를 복사할 수 있어야 한다.
    expect(shown).toContain("경로 복사");
  });

  it("가드에 걸리면 변경 항목만 비활성이 되고 사유가 툴팁에 붙는다", () => {
    render({ block: { taskId: 12, reason: "작업 #12가 이 워크트리를 쓰고 있습니다" } });
    const byLabel = (text: string) => items().find((b) => b.textContent === text)!;

    const trash = byLabel("휴지통으로 이동");
    expect(trash.disabled).toBe(true);
    expect(trash.getAttribute("aria-disabled")).toBe("true");
    expect(trash.title).toContain("#12");

    // 읽기 계열은 가드 대상이 아니다.
    const terminal = byLabel("터미널에서 열기");
    expect(terminal.disabled).toBe(false);
    // 클립보드에 담기만 하는 "복사"도 쓰기가 아니라 막지 않는다.
    expect(byLabel("복사").disabled).toBe(false);
  });

  it("가드에 걸린 항목은 클릭해도 액션이 나가지 않는다", () => {
    const onAction = vi.fn();
    render({ block: { taskId: 3, reason: "작업 #3" }, onAction });
    const trash = items().find((b) => b.textContent === "휴지통으로 이동")!;
    act(() => trash.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(onAction).not.toHaveBeenCalled();
  });

  it("메뉴가 없으면 아무것도 그리지 않는다", () => {
    act(() => {
      root.render(
        <FileContextMenu
          menu={null}
          block={null}
          readOnly={false}
          clipboard={null}
          onAction={() => {}}
          onClose={() => {}}
        />,
      );
    });
    expect(container.querySelector('[role="menu"]')).toBeNull();
  });
});
