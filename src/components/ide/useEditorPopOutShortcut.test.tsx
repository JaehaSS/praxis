// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useEditorPopOutShortcut } from "./useEditorPopOutShortcut";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;
let onToggle: ReturnType<typeof vi.fn>;

function Harness({ active }: { active: boolean }) {
  useEditorPopOutShortcut({ active: () => active, onToggle });
  return null;
}

const render = async (active = true): Promise<void> => {
  await act(async () => {
    root?.render(<Harness active={active} />);
  });
};

const press = async (init: KeyboardEventInit): Promise<void> => {
  await act(async () => {
    window.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, code: "KeyE", key: "e", ...init }),
    );
  });
};

beforeEach(() => {
  onToggle = vi.fn();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  container = null;
  root = null;
});

describe("에디터 팝아웃 단축키", () => {
  it("⌥⌘E로 토글을 부른다", async () => {
    await render();
    await press({ metaKey: true, altKey: true });

    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("워크스페이스 밖에서는 듣지 않는다", async () => {
    // 뺄 에디터가 없는 화면에서 창을 여는 것은 사용자가 요청한 적 없는 이동이다.
    await render(false);
    await press({ metaKey: true, altKey: true });

    expect(onToggle).not.toHaveBeenCalled();
  });

  it("⌥ 없는 ⌘E와 ⇧를 섞은 조합은 지나 보낸다", async () => {
    await render();
    await press({ metaKey: true });
    await press({ metaKey: true, altKey: true, shiftKey: true });

    expect(onToggle).not.toHaveBeenCalled();
  });

  it("앞에서 이미 처리한 키는 다시 처리하지 않는다", async () => {
    // `defaultPrevented`는 모달·에디터가 이미 그 키를 썼다는 뜻이다 — 위에서 가로챈 것을
    // 전역 리스너가 한 번 더 해석하면 같은 입력이 두 가지 일을 한다.
    await render();
    await act(async () => {
      const event = new KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        code: "KeyE",
        key: "e",
        metaKey: true,
        altKey: true,
      });
      event.preventDefault();
      window.dispatchEvent(event);
    });

    expect(onToggle).not.toHaveBeenCalled();
  });

  it("눌러 둔 채의 반복 keydown은 무시한다", async () => {
    // 창 열기는 IPC다 — 연타로 보내면 같은 창을 여러 번 띄우려 한다.
    await render();
    await press({ metaKey: true, altKey: true, repeat: true });

    expect(onToggle).not.toHaveBeenCalled();
  });

  it("언마운트하면 리스너를 걷는다", async () => {
    await render();
    await act(async () => root?.unmount());

    await press({ metaKey: true, altKey: true });

    expect(onToggle).not.toHaveBeenCalled();
  });
});
