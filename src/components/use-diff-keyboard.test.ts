// @vitest-environment jsdom

import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import {
  nextFileIndex,
  nextHunkIndex,
  shouldHandleDiffKey,
  useDiffKeyboard,
  type DiffKeyHandlers,
} from "./use-diff-keyboard";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("nextFileIndex", () => {
  it("moves within bounds", () => {
    expect(nextFileIndex(1, 4, 1)).toBe(2);
    expect(nextFileIndex(1, 4, -1)).toBe(0);
  });

  it("clamps at both ends instead of wrapping", () => {
    expect(nextFileIndex(3, 4, 1)).toBe(3);
    expect(nextFileIndex(0, 4, -1)).toBe(0);
  });

  it("starts at the first file when nothing is selected — either direction", () => {
    expect(nextFileIndex(-1, 4, 1)).toBe(0);
    expect(nextFileIndex(-1, 4, -1)).toBe(0);
  });

  it("reports no target for an empty list", () => {
    expect(nextFileIndex(-1, 0, 1)).toBe(-1);
  });
});

describe("nextHunkIndex", () => {
  // 컨테이너 상단 = 0. 음수 top은 이미 지나친 헤더다.
  it("jumps to the first header below the viewport top", () => {
    expect(nextHunkIndex([-50, 40, 120], 0, 1)).toBe(1);
  });

  it("goes back to the header before the current one", () => {
    expect(nextHunkIndex([-90, -50, 120], 0, -1)).toBe(0);
  });

  it("clamps at the last header when everything is above", () => {
    expect(nextHunkIndex([-90, -50], 0, 1)).toBe(1);
  });

  it("clamps at the first header when nothing is above", () => {
    expect(nextHunkIndex([40, 120], 0, -1)).toBe(0);
  });

  it("handles a single header", () => {
    expect(nextHunkIndex([10], 0, 1)).toBe(0);
    expect(nextHunkIndex([10], 0, -1)).toBe(0);
  });

  it("reports no target when there are no headers", () => {
    expect(nextHunkIndex([], 0, 1)).toBe(-1);
  });
});

describe("useDiffKeyboard dispatch", () => {
  const spies = () => ({
    nextHunk: vi.fn(),
    prevHunk: vi.fn(),
    nextFile: vi.fn(),
    prevFile: vi.fn(),
    setMode: vi.fn(),
    toggleViewed: vi.fn(),
  });

  async function mount(handlers: DiffKeyHandlers) {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const Probe = () => {
      useDiffKeyboard(handlers);
      return null;
    };
    await act(async () => root.render(createElement(Probe)));
    return async () => {
      await act(async () => root.unmount());
      container.remove();
    };
  }

  const press = async (key: string, init: KeyboardEventInit = {}) => {
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key, ...init }));
    });
  };

  it("routes each mapped key to its handler exactly once", async () => {
    const handlers = spies();
    const unmount = await mount(handlers);

    await press("j");
    await press("k");
    await press("]");
    await press("[");
    await press("v");

    expect(handlers.nextHunk).toHaveBeenCalledTimes(1);
    expect(handlers.prevHunk).toHaveBeenCalledTimes(1);
    expect(handlers.nextFile).toHaveBeenCalledTimes(1);
    expect(handlers.prevFile).toHaveBeenCalledTimes(1);
    expect(handlers.toggleViewed).toHaveBeenCalledTimes(1);
    await unmount();
  });

  it("maps u and s to the two modes", async () => {
    const handlers = spies();
    const unmount = await mount(handlers);
    await press("u");
    await press("s");
    expect(handlers.setMode.mock.calls).toEqual([["unified"], ["split"]]);
    await unmount();
  });

  it("ignores unmapped keys", async () => {
    const handlers = spies();
    const unmount = await mount(handlers);
    await press("x");
    expect(Object.values(handlers).every((spy) => spy.mock.calls.length === 0)).toBe(true);
    await unmount();
  });

  it("ignores Shift combinations — key.toLowerCase() would read Shift+J as j", async () => {
    const handlers = spies();
    const unmount = await mount(handlers);
    await press("J", { shiftKey: true });
    expect(handlers.nextHunk).not.toHaveBeenCalled();
    await unmount();
  });

  it("stops listening after unmount", async () => {
    const handlers = spies();
    const unmount = await mount(handlers);
    await unmount();
    await press("j");
    expect(handlers.nextHunk).not.toHaveBeenCalled();
  });
});

describe("shouldHandleDiffKey", () => {
  it("handles keys when focus is on the body", () => {
    expect(shouldHandleDiffKey(document.body)).toBe(true);
  });

  it("ignores keys while the annotation composer has focus", () => {
    expect(shouldHandleDiffKey(document.createElement("textarea"))).toBe(false);
  });

  it("ignores keys inside a text input", () => {
    expect(shouldHandleDiffKey(document.createElement("input"))).toBe(false);
  });

  it("ignores keys inside a select", () => {
    expect(shouldHandleDiffKey(document.createElement("select"))).toBe(false);
  });

  it("ignores keys inside contenteditable regions", () => {
    const editable = document.createElement("div");
    editable.contentEditable = "true";
    // jsdom은 contentEditable을 반영하지 않으므로 isContentEditable을 직접 정의한다.
    Object.defineProperty(editable, "isContentEditable", { value: true });
    expect(shouldHandleDiffKey(editable)).toBe(false);
  });

  it("handles keys when the target is not an element at all", () => {
    expect(shouldHandleDiffKey(null)).toBe(true);
  });
});
